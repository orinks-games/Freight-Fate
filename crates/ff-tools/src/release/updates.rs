//! What the in-game updater reads from a release: each build's update
//! identity, baked in by `build-release`, and the signed manifests and file
//! indexes `update-manifests` writes beside the release's archives.
//!
//! The updater is the engine's (`hearken-update`). A copy trusts only
//! manifests signed with the game's update key, whose public half is
//! `update-key` under `[workspace.metadata.release]` in the root
//! Cargo.toml. The secret is never in the repository: it is a file in the
//! developer's config folder (`update-key --new` makes it) or the
//! `HEARKEN_UPDATE_SECRET` secret on CI.

use std::path::{Path, PathBuf};

use hearken_update::{manifest, IndexRef, Manifest, PublicKey, SecretKey, Version};

use super::{is_snapshot_label, release_metadata};
use crate::cli::{Args, Spec};
use crate::util::{self, Result};

/// The package name every manifest is signed for, as the game checks it.
pub const GAME: &str = "freight-fate";

/// Where CI passes the update key's secret.
pub const SECRET_VAR: &str = "HEARKEN_UPDATE_SECRET";

/// The game's committed public update key, or `None` before one is made.
pub fn committed_key() -> Result<Option<PublicKey>> {
    match release_metadata("update-key")?.filter(|k| !k.is_empty()) {
        None => Ok(None),
        Some(text) => PublicKey::parse(&text).map(Some).ok_or_else(|| {
            format!("update-key {text:?} is not a public key: it is ed25519: and 64 hex digits.")
        }),
    }
}

/// The channel a build labelled `label` follows.
pub fn channel_of(label: &str) -> &'static str {
    if is_snapshot_label(label) {
        "dev"
    } else {
        "stable"
    }
}

/// The version the updater compares a build labelled `label` by.
///
/// A stable release is its tag's version, which must be a plain
/// `MAJOR.MINOR.PATCH` (a hotfix is the next patch, not a fourth number).
/// A snapshot is the next patch after the Cargo version, as a pre-release
/// stamped with its commit's UTC time: `1.9.6-dev.202610300537`. Every job
/// of one release builds the same commit, so they all agree on it, and a
/// newer snapshot always sorts above an older one and below the stable it
/// leads to.
pub fn update_version(label: &str, crate_version: &str, commit_stamp: &str) -> Result<String> {
    if is_snapshot_label(label) {
        let base = Version::parse(crate_version)
            .ok_or_else(|| format!("the Cargo version {crate_version} is not a version"))?;
        let mut parts = crate_version
            .split(['-', '+'])
            .next()
            .unwrap_or("")
            .split('.');
        let major = parts.next().unwrap_or("0");
        let minor = parts.next().unwrap_or("0");
        let patch: u64 = parts
            .next()
            .and_then(|p| p.parse().ok())
            .ok_or_else(|| format!("the Cargo version {base:?} has no patch number"))?;
        if commit_stamp.len() != 12 || !commit_stamp.bytes().all(|b| b.is_ascii_digit()) {
            return Err(format!(
                "the commit time {commit_stamp:?} is not YYYYMMDDHHMM"
            ));
        }
        let version = format!("{major}.{minor}.{}-dev.{commit_stamp}", patch + 1);
        return Version::parse(&version)
            .map(|_| version.clone())
            .ok_or_else(|| format!("{version} is not a version"));
    }
    let version = label.strip_prefix('v').unwrap_or(label);
    let plain = version.split('.').count() == 3 && !version.contains(['-', '+']);
    if !plain || Version::parse(version).is_none() {
        return Err(format!(
            "The release {label} cannot be offered as an update: a release is \
             MAJOR.MINOR.PATCH, so a hotfix of 1.9.5 is 1.9.6, not 1.9.5.1."
        ));
    }
    Ok(version.to_string())
}

/// The checked-out commit's time, UTC, as `YYYYMMDDHHMM`.
pub fn commit_stamp() -> Result<String> {
    let mut cmd = std::process::Command::new("git");
    cmd.args([
        "log",
        "-1",
        "--format=%cd",
        "--date=format-local:%Y%m%d%H%M",
    ])
    .env("TZ", "UTC")
    .current_dir(util::repo_root());
    let out = cmd
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if !out.status.success() {
        return Err("git log failed: the update version needs the commit's time".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The `HEARKEN_UPDATE_*` variables `cargo build` bakes into a release
/// built as `label`, or nothing (with a warning) before the game has an
/// update key: that build never updates itself.
pub fn bake_vars(label: &str, crate_version: &str) -> Result<Vec<(&'static str, String)>> {
    let Some(key) = committed_key()? else {
        println!(
            "warning: no update-key in Cargo.toml's [workspace.metadata.release]; \
             this build will not update itself."
        );
        return Ok(Vec::new());
    };
    let version = update_version(label, crate_version, &commit_stamp()?)?;
    Ok(vec![
        ("HEARKEN_UPDATE_KEY", key.to_string()),
        ("HEARKEN_UPDATE_GAME", GAME.to_string()),
        ("HEARKEN_UPDATE_VERSION", version),
        ("HEARKEN_UPDATE_CHANNEL", channel_of(label).to_string()),
    ])
}

/// The updater's platform for a release archive, by the name the release
/// gives it; `None` for files that are not a game archive.
pub fn platform_of(archive: &str) -> Option<&'static str> {
    if !archive.starts_with("FreightFate-") {
        return None;
    }
    const SUFFIXES: &[(&str, &str)] = &[
        ("-windows-portable.zip", "windows-x64"),
        ("-macos-arm64.zip", "macos-arm64"),
        ("-linux-x64.tar.gz", "linux-x64"),
        ("-linux-arm64.tar.gz", "linux-arm64"),
        ("-linux-x86_64.AppImage", "linux-x64-appimage"),
        ("-linux-aarch64.AppImage", "linux-arm64-appimage"),
    ];
    SUFFIXES
        .iter()
        .find(|(suffix, _)| archive.ends_with(suffix))
        .map(|(_, platform)| *platform)
}

/// The update key's secret: `HEARKEN_UPDATE_SECRET`, else the key file.
fn secret_key() -> Result<SecretKey> {
    if let Some(hex) = std::env::var(SECRET_VAR)
        .ok()
        .filter(|v| !v.trim().is_empty())
    {
        return SecretKey::from_hex(hex.trim())
            .ok_or_else(|| format!("{SECRET_VAR} must be 64 hexadecimal digits."));
    }
    let path = key_file()?;
    let text = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "No update key: set {SECRET_VAR}, or restore {} from your backup.",
            path.display()
        )
    })?;
    read_key_file(&text).ok_or_else(|| format!("{} is not an update key file.", path.display()))
}

/// The key in a key file: its one line that is not blank or a `#` comment.
fn read_key_file(text: &str) -> Option<SecretKey> {
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'));
    let key = SecretKey::from_hex(lines.next()?)?;
    lines.next().is_none().then_some(key)
}

/// The key file: `hearken/update-keys/freight-fate.key` in the config
/// folder, where Hearken's own tools keep a game's update key.
fn key_file() -> Result<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    let base = base.ok_or("There is no config folder to keep the update key in.")?;
    Ok(base
        .join("hearken")
        .join("update-keys")
        .join(format!("{GAME}.key")))
}

/// The manifest's notes: the release notes file, if any.
fn read_notes(path: Option<&Path>) -> Result<String> {
    let Some(path) = path else {
        return Ok(String::new());
    };
    let text = util::read_text(path)?;
    if text.len() > manifest::MAX_NOTES_BYTES {
        // Too long for the in-game reader's manifest; the release page
        // still has them all.
        let mut cut = manifest::MAX_NOTES_BYTES - 200;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        return Ok(format!(
            "{}\n\nThe rest is on the release page.",
            &text[..cut]
        ));
    }
    Ok(text)
}

fn sha256_of(path: &Path) -> Result<(u64, String)> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut size = 0u64;
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hasher.update(&buf[..n]);
    }
    Ok((size, hex::encode(hasher.finalize())))
}

/// Writes, beside each archive in `dir`, its file index (for a zip) and
/// its signed manifest, `update-<channel>-<platform>.json`. Returns the
/// files written.
pub fn write_manifests(
    dir: &Path,
    label: &str,
    version: &str,
    notes: &str,
    secret: &SecretKey,
) -> Result<Vec<PathBuf>> {
    let channel = channel_of(label);
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| platform_of(n).is_some())
        .collect();
    names.sort();
    if names.is_empty() {
        return Err(format!("{} holds no release archive", dir.display()));
    }
    let mut written = Vec::new();
    let mut platforms = Vec::new();
    for name in names {
        let platform = platform_of(&name).expect("filtered");
        if platforms.contains(&platform) {
            return Err(format!("two archives for {platform} in {}", dir.display()));
        }
        platforms.push(platform);
        let archive = dir.join(&name);
        let (size, sha256) = sha256_of(&archive)?;
        let index = match hearken_update::index::index_name(&name) {
            Some(file) => {
                let index = hearken_update::index::build(&archive)
                    .map_err(|e| format!("could not index {name}: {e}"))?;
                let text = index.to_json();
                if text.len() as u64 > hearken_update::index::MAX_INDEX_BYTES {
                    return Err(format!("{name} holds too many files to index"));
                }
                let path = dir.join(&file);
                util::write_text(&path, &text)?;
                let (index_size, index_sha) = sha256_of(&path)?;
                written.push(path);
                Some(IndexRef {
                    file,
                    size: index_size,
                    sha256: index_sha,
                })
            }
            None => None,
        };
        let manifest = Manifest {
            format: 1,
            game: GAME.to_string(),
            version: version.to_string(),
            channel: channel.to_string(),
            platform: platform.to_string(),
            archive: name.clone(),
            url: None,
            size,
            sha256,
            notes: notes.to_string(),
            published: Some(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
            index,
        };
        let text = manifest::sign(&manifest, secret)
            .map_err(|e| format!("could not sign the manifest for {name}: {e}"))?;
        let path = dir.join(format!("update-{channel}-{platform}.json"));
        util::write_text(&path, &text)?;
        written.push(path);
    }
    Ok(written)
}

const USAGE: &str = "usage: update-manifests --dir DIR --tag LABEL [--notes FILE]";

/// `update-manifests`: sign the release's update manifests and write the
/// zips' file indexes, beside its archives in `--dir`.
pub fn manifests_main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["dir", "tag", "notes"],
            flags: &[],
        },
    )
    .map_err(|e| format!("{e}\n{USAGE}"))?;
    let dir = args.path("dir").ok_or(USAGE)?;
    let label = args.value("tag").filter(|t| !t.is_empty()).ok_or(USAGE)?;
    let Some(public) = committed_key()? else {
        println!("No update-key in Cargo.toml yet: no update manifests written.");
        return Ok(0);
    };
    let secret = secret_key()?;
    if secret.public() != public {
        return Err(format!(
            "The update secret is for {}, but Cargo.toml's update-key is {public}. \
             Players' copies would refuse every update it signed.",
            secret.public()
        ));
    }
    let version = update_version(label, &super::crate_version()?, &commit_stamp()?)?;
    let notes = read_notes(args.path("notes").as_deref())?;
    let written = write_manifests(&dir, label, &version, &notes, &secret)?;
    println!("Update {version} on {}:", channel_of(label));
    for path in written {
        println!("  {}", path.display());
    }
    Ok(0)
}

/// `update-key`: where the update key is and whether it matches; `--new`
/// makes it (once), `--print` shows the secret for the CI secret.
pub fn key_main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &[],
            flags: &["new", "print"],
        },
    )
    .map_err(|e| format!("{e}\nusage: update-key [--new] [--print]"))?;
    let path = key_file()?;
    if args.flag("new") {
        if committed_key()?.is_some() {
            return Err(
                "Cargo.toml already has an update-key. Players' copies trust only that key, \
                 so a new one would never reach them; none is made."
                    .into(),
            );
        }
        if path.exists() {
            return Err(format!("{} already exists.", path.display()));
        }
        let key = SecretKey::generate().map_err(|e| format!("could not make a key: {e}"))?;
        let text = format!(
            "# The update key of {GAME}: its update manifests are signed with it.\n\
             # Keep a backup. Without it, players' copies accept no later update.\n\
             # Never commit it. Its public half, safe to commit, is {}.\n{}\n",
            key.public(),
            key.to_hex()
        );
        write_private(&path, &text)?;
        println!(
            "Made the update key in\n    {}\n\nBack it up somewhere safe. Then add this line to \
             [workspace.metadata.release] in Cargo.toml and commit it:\n\n    \
             update-key = \"{}\"\n\nFor CI, store `cargo tools update-key --print`'s secret as \
             the repository secret {SECRET_VAR}.",
            path.display(),
            key.public()
        );
        return Ok(0);
    }
    let key = secret_key()?;
    println!("Public key: {}", key.public());
    match committed_key()? {
        Some(public) if public == key.public() => println!("It matches Cargo.toml's update-key."),
        Some(public) => println!("It does NOT match Cargo.toml's update-key, {public}."),
        None => println!("Cargo.toml has no update-key yet."),
    }
    if args.flag("print") {
        println!("Secret: {}", key.to_hex());
    }
    Ok(0)
}

/// Writes a new file only its owner can read. Never overwrites one.
fn write_private(path: &Path, text: &str) -> Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(text.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_releases_are_their_tag_and_hotfixes_must_be_a_patch() {
        assert_eq!(update_version("v1.9.6", "1.9.5", "").unwrap(), "1.9.6");
        assert_eq!(update_version("1.9.6", "1.9.5", "").unwrap(), "1.9.6");
        let four = update_version("v1.9.5.1", "1.9.5", "").unwrap_err();
        assert!(four.contains("not 1.9.5.1"), "{four}");
        assert!(update_version("v1.9.6-beta", "1.9.5", "").is_err());
    }

    #[test]
    fn a_snapshot_is_the_next_patch_stamped_with_its_commit() {
        assert_eq!(
            update_version("1.9-tester-20261030", "1.9.5", "202610300537").unwrap(),
            "1.9.6-dev.202610300537"
        );
        assert!(update_version("1.9-tester-20261030", "1.9.5", "2026").is_err());
        let v = |s: &str| Version::parse(s).unwrap();
        assert!(v("1.9.6-dev.202610300537").is_newer_than(&v("1.9.5")));
        assert!(v("1.9.6").is_newer_than(&v("1.9.6-dev.202610300537")));
    }

    #[test]
    fn each_archive_has_its_platform() {
        let cases = [
            ("FreightFate-v1.9.6-windows-portable.zip", "windows-x64"),
            ("FreightFate-v1.9.6-macos-arm64.zip", "macos-arm64"),
            ("FreightFate-v1.9.6-linux-x64.tar.gz", "linux-x64"),
            ("FreightFate-v1.9.6-linux-arm64.tar.gz", "linux-arm64"),
            (
                "FreightFate-v1.9.6-linux-x86_64.AppImage",
                "linux-x64-appimage",
            ),
            (
                "FreightFate-v1.9.6-linux-aarch64.AppImage",
                "linux-arm64-appimage",
            ),
        ];
        for (name, platform) in cases {
            assert_eq!(platform_of(name), Some(platform), "{name}");
        }
        assert_eq!(platform_of("FreightFate-v1.9.6-translation-kit.zip"), None);
        assert_eq!(platform_of("checksums.txt"), None);
        assert_eq!(platform_of("FreightFate-v1.9.6-macos.zip"), None);
    }

    #[test]
    fn the_manifests_are_signed_and_name_their_archives() {
        let dir = tempfile::tempdir().unwrap();
        // A zip with one file, and a tarball's worth of bytes.
        let zip_path = dir.path().join("FreightFate-v1.9.6-windows-portable.zip");
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file::<_, ()>(
                "FreightFate/FreightFate.exe",
                zip::write::FileOptions::default(),
            )
            .unwrap();
            std::io::Write::write_all(&mut zip, b"MZ program").unwrap();
            zip.finish().unwrap();
        }
        std::fs::write(
            dir.path().join("FreightFate-v1.9.6-linux-x64.tar.gz"),
            b"tar",
        )
        .unwrap();
        std::fs::write(dir.path().join("checksums.txt"), b"x").unwrap();
        let secret = SecretKey::generate().unwrap();

        let written = write_manifests(dir.path(), "v1.9.6", "1.9.6", "- Faster.", &secret).unwrap();

        let names: Vec<String> = written
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.contains(&"update-stable-windows-x64.json".to_string()),
            "{names:?}"
        );
        assert!(
            names.contains(&"update-stable-linux-x64.json".to_string()),
            "{names:?}"
        );
        let windows = std::fs::read(dir.path().join("update-stable-windows-x64.json")).unwrap();
        let opened = manifest::open(&windows, &secret.public()).unwrap();
        assert_eq!(opened.game, "freight-fate");
        assert_eq!(opened.version, "1.9.6");
        assert_eq!(opened.archive, "FreightFate-v1.9.6-windows-portable.zip");
        assert_eq!(opened.notes, "- Faster.");
        let index = opened.index.expect("a zip has a file index");
        assert!(dir.path().join(&index.file).is_file());
        let linux = std::fs::read(dir.path().join("update-stable-linux-x64.json")).unwrap();
        assert_eq!(
            manifest::open(&linux, &secret.public()).unwrap().index,
            None
        );
        // Another key's copies refuse it.
        let other = SecretKey::generate().unwrap();
        assert!(manifest::open(&windows, &other.public()).is_err());
    }

    #[test]
    fn a_key_file_holds_one_key() {
        let key = SecretKey::generate().unwrap();
        let text = format!("# comment\n{}\n", key.to_hex());
        assert_eq!(read_key_file(&text).unwrap().public(), key.public());
        assert!(read_key_file(&format!("{0}\n{0}\n", key.to_hex())).is_none());
    }
}
