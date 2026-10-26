//! The sound, music and Channel 3000 packs: refusing a Git LFS pointer,
//! fetching the pinned public packs, and proving a staged pack carries what
//! the game reads.

use std::path::{Path, PathBuf};

use crate::http;
use crate::util::{self, Result};

/// Production, so a release no longer pulls its music through the staging
/// host. The route is a 307 to FREIGHT_FATE_MUSIC_BLOB_URL; that Vercel
/// variable must be on the Production scope (it is as of 2026-09-20), and
/// Vercel bakes the environment at deploy time, so setting it takes a
/// redeploy. The sha256 below is what actually gates the download either way.
const DEFAULT_MUSIC_URL: &str = "https://www.orinks.net/downloads/music.pak";
const DEFAULT_MUSIC_SHA256: &str =
    "c56401a2a45057faba0bd4bf7d024ce7dd3908f682f4c35526643a1870cb7798";
/// Channel 3000's clips, packed from the owner's local clips folder. Its own
/// pack rather than more of music.pak, which the game reads whole into
/// memory; the game opens this one only when the radio is first tuned to
/// 87.7. Each rebuild is published under a new name beside the old one, so a
/// branch still pinned to an older pack keeps building; a rebuild moves both
/// lines.
const DEFAULT_CHANNEL3000_URL: &str =
    "https://crisp-crystal-9a9y.here.now/channel3000-a8343d91.pak";
const DEFAULT_CHANNEL3000_SHA256: &str =
    "a8343d916fdd31083a46497dd393865cbc78f4045faa9c6ef42f817f684dd179";
/// Git LFS writes this text in place of a pack that was never fetched.
const LFS_POINTER_PREFIX: &[u8] = b"version https://git-lfs";

fn assets() -> PathBuf {
    util::repo_root().join(super::SOURCE_ASSETS)
}

/// True when `path` is a Git LFS pointer rather than the real file.
pub fn is_lfs_pointer(path: &Path) -> bool {
    let Ok(meta) = path.metadata() else {
        return false;
    };
    if meta.len() > 1024 {
        return false;
    }
    std::fs::read(path).is_ok_and(|b| b.starts_with(LFS_POINTER_PREFIX))
}

/// Refuse to stage a pack that is only a Git LFS pointer.
pub fn require_real_pack(path: &Path) -> Result<()> {
    if !path.exists() {
        return Err(format!("Sound pack was not found: {}", path.display()));
    }
    if is_lfs_pointer(path) {
        return Err(format!(
            "{} is a Git LFS pointer, not the pack itself; run `git lfs pull` in this \
             checkout before building",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
    }
    Ok(())
}

/// `<prefix>_URL` and `<prefix>_SHA256` from the environment, else the pins.
fn pinned_config(prefix: &str, url: &str, sha256: &str) -> Result<(String, String)> {
    let url = std::env::var(format!("{prefix}_URL")).unwrap_or_else(|_| url.to_string());
    let sha = std::env::var(format!("{prefix}_SHA256"))
        .unwrap_or_else(|_| sha256.to_string())
        .to_lowercase();
    if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!(
            "{prefix}_SHA256 must be a 64-character hexadecimal digest"
        ));
    }
    Ok((url, sha))
}

/// Download `url` to `path` unless a copy with the pinned digest is there.
/// The download lands in a temporary file beside `path` and replaces it only
/// once its digest matches, so a failed or tampered download never costs the
/// copy already there.
fn ensure_pinned_pack(path: &Path, url: &str, expected: &str, label: &str) -> Result<()> {
    if path.is_file() && !is_lfs_pointer(path) && util::file_sha256(path)? == expected {
        return Ok(());
    }
    let parent = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let mut temp = tempfile::Builder::new()
        .prefix(&format!("{name}."))
        .suffix(".download")
        .tempfile_in(parent)
        .map_err(|e| e.to_string())?;
    let agent = http::agent(None);
    let status = http::download(
        &agent,
        url,
        &[("User-Agent", "Freight-Fate-release-builder/1.9")],
        temp.as_file_mut(),
    )
    .map_err(|detail| {
        format!("{label} download failed: {detail}. Check your connection and retry the build.")
    })?;
    if !(200..300).contains(&status) {
        return Err(format!(
            "{label} download failed with HTTP status {status}. Check your connection and \
             retry the build."
        ));
    }
    let actual = util::file_sha256(temp.path())?;
    if actual != expected {
        return Err(format!(
            "Downloaded {name} failed SHA-256 verification: expected {expected}, got {actual}"
        ));
    }
    temp.persist(path).map_err(|e| e.to_string())?;
    println!("Downloaded and verified {name} ({actual}).");
    Ok(())
}

/// Download and verify the public music pack when it is not already present.
pub fn ensure_music_pack(path: Option<&Path>) -> Result<()> {
    let (url, sha) = pinned_config(
        "FREIGHT_FATE_MUSIC",
        DEFAULT_MUSIC_URL,
        DEFAULT_MUSIC_SHA256,
    )?;
    let path = path.map_or_else(|| assets().join("music.pak"), Path::to_path_buf);
    ensure_pinned_pack(&path, &url, &sha, "Music-pack")
}

/// Download and verify Channel 3000's pack when it is not already present.
pub fn ensure_channel3000_pack(path: Option<&Path>) -> Result<()> {
    let (url, sha) = pinned_config(
        "FREIGHT_FATE_CHANNEL3000",
        DEFAULT_CHANNEL3000_URL,
        DEFAULT_CHANNEL3000_SHA256,
    )?;
    let path = path.map_or_else(|| assets().join("channel3000.pak"), Path::to_path_buf);
    ensure_pinned_pack(&path, &url, &sha, "Channel 3000 pack")
}

#[cfg(feature = "world")]
fn pack_names(path: &Path) -> Result<Vec<String>> {
    ff_core::assets_pack::SoundPack::open(path)
        .map(|p| p.names())
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(not(feature = "world"))]
fn pack_names(_path: &Path) -> Result<Vec<String>> {
    Err("this build of ff-tools has no `world` feature, so it cannot read sound packs".into())
}

/// Open the staged packs and prove they carry what the game reads.
pub fn verify_sound_packs(build: &Path) -> Result<()> {
    let root = if build.extension().is_some_and(|e| e == "app") {
        build.join("Contents").join("Resources")
    } else {
        build.to_path_buf()
    };
    let package = root.join("freight_fate");
    let names = pack_names(&package.join("sounds.pak"))?;
    if !names
        .iter()
        .any(|n| n.ends_with(".ogg") || n.ends_with(".wav"))
    {
        return Err("Packaged sound pack contains no audio files".into());
    }
    if !names.iter().any(|n| n == "engine_classic/idle.ogg") {
        return Err(
            "Packaged sound pack predates the classic engine voice, so the Settings \
             engine-voice option would fall back silently; rebuild the committed pack with \
             `cargo tools pack-sounds` on a builder machine and commit it"
                .into(),
        );
    }
    if !pack_names(&package.join("music.pak"))?
        .iter()
        .any(|n| n.starts_with("music/"))
    {
        return Err("Packaged music pack contains no music files".into());
    }
    if !pack_names(&package.join("channel3000.pak"))?
        .iter()
        .any(|n| n.starts_with("c3k/"))
    {
        return Err("Packaged Channel 3000 pack contains no clips".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lfs_pointers_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let pointer = dir.path().join("sounds.pak");
        std::fs::write(
            &pointer,
            "version https://git-lfs.github.com/spec/v1\noid sha256:x\n",
        )
        .unwrap();
        assert!(is_lfs_pointer(&pointer));
        assert!(require_real_pack(&pointer)
            .unwrap_err()
            .contains("Git LFS pointer"));
        assert!(require_real_pack(&dir.path().join("missing.pak")).is_err());
    }

    #[test]
    fn a_matching_pack_is_not_downloaded_again() {
        let dir = tempfile::tempdir().unwrap();
        let pack = dir.path().join("x.pak");
        std::fs::write(&pack, b"abc").unwrap();
        let sha = util::file_sha256(&pack).unwrap();
        // An unroutable URL: reaching the network would fail the test.
        ensure_pinned_pack(&pack, "http://127.0.0.1:9/never", &sha, "Test").unwrap();
    }
}
