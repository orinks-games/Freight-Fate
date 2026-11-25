//! The sound, music and Channel 3000 packs: refusing a Git LFS pointer,
//! fetching the pinned public packs, and proving a staged pack carries what
//! the game reads.

use std::path::{Path, PathBuf};

use crate::http;
use crate::util::{self, Result};

/// The music pack. Each rebuild is published under a new name beside the
/// old one (`music-<first 8 hex of its SHA-256>.pak`), so a branch still
/// pinned to an older pack keeps building; a rebuild moves both lines. The
/// 2.0 pack is the 1.9 pack plus `assets/radio-2.0/pack-overlay/`, and that
/// folder's README has the rebuild command. The SHA-256 is what gates the
/// download.
const DEFAULT_MUSIC_URL: &str = "https://crisp-crystal-9a9y.here.now/music-9f4c05a1.pak";
const DEFAULT_MUSIC_SHA256: &str =
    "9f4c05a19b3d0363abff7607cdc1648a08ca36d24a5a6dcce0a60cab0d51aa31";
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
/// The licensed stereo cab engine cuts, packed alone with `cargo tools
/// pack-sounds --licensed-out` on the owner's machine, which is the only
/// place the purchased recordings live. Staging fetches it when there is no
/// `assets/sounds-licensed` folder, so a CI builder can package. Both empty
/// means no pack is published yet and staging behaves as if this did not
/// exist; `FREIGHT_FATE_LICENSED_PACK_URL` and `_SHA256` override them.
/// Publish each rebuild under a new name, as with Channel 3000.
#[cfg(feature = "world")]
const DEFAULT_LICENSED_SOUNDS_URL: &str = "";
#[cfg(feature = "world")]
const DEFAULT_LICENSED_SOUNDS_SHA256: &str = "";
#[cfg(feature = "world")]
const LICENSED_PACK_ENV: &str = "FREIGHT_FATE_LICENSED_PACK";
/// Where the downloaded licensed pack is kept (gitignored).
#[cfg(feature = "world")]
const LICENSED_PACK_FILE: &str = "sounds-licensed.pak";
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

/// The licensed pack's address and digest, or `None` while none is pinned.
#[cfg(feature = "world")]
fn licensed_pin() -> Result<Option<(String, String)>> {
    let env = |suffix: &str, default: &str| {
        std::env::var(format!("{LICENSED_PACK_ENV}_{suffix}"))
            .unwrap_or_else(|_| default.to_string())
    };
    let url = env("URL", DEFAULT_LICENSED_SOUNDS_URL);
    let sha = env("SHA256", DEFAULT_LICENSED_SOUNDS_SHA256);
    match (url.trim().is_empty(), sha.trim().is_empty()) {
        (true, true) => Ok(None),
        (false, false) => pinned_config(
            LICENSED_PACK_ENV,
            DEFAULT_LICENSED_SOUNDS_URL,
            DEFAULT_LICENSED_SOUNDS_SHA256,
        )
        .map(Some),
        _ => Err(format!(
            "The licensed engine pack needs both an address and a SHA-256 \
             ({LICENSED_PACK_ENV}_URL and {LICENSED_PACK_ENV}_SHA256, or the pins in \
             release/packs.rs)"
        )),
    }
}

/// Download and verify the licensed engine pack into `assets_dir` when one
/// is pinned. `None` when nothing is pinned.
#[cfg(feature = "world")]
pub fn ensure_licensed_sounds_pack(assets_dir: &Path) -> Result<Option<PathBuf>> {
    let Some((url, sha)) = licensed_pin()? else {
        return Ok(None);
    };
    let path = assets_dir.join(LICENSED_PACK_FILE);
    ensure_pinned_pack(&path, &url, &sha, "Licensed engine pack")?;
    Ok(Some(path))
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

/// The calibrated stereo cab engine startup, shutdown, and running loops
/// required in every licensed release `sounds.pak`.
pub const REQUIRED_LICENSED_ENGINE_ENTRIES: &[&str] = &[
    "engine_stereo/start.wav",
    "engine_stereo/shutdown.wav",
    "engine_stereo/idle.wav",
    "engine_stereo/low.wav",
    "engine_stereo/mid.wav",
    "engine_stereo/midhigh.wav",
    "engine_stereo/high.wav",
];

/// Return any required licensed stereo cab engine entries missing from `names`.
pub fn missing_licensed_engine_entries(names: &[String]) -> Vec<&'static str> {
    REQUIRED_LICENSED_ENGINE_ENTRIES
        .iter()
        .copied()
        .filter(|req| !names.iter().any(|n| n == req))
        .collect()
}

/// Stage the runtime `sounds.pak` by overlaying the private licensed sound tree
/// (`FREIGHT_FATE_LICENSED_SOUNDS` or `<assets>/sounds-licensed`) onto the base
/// `assets/sounds.pak` when present, never touching the tracked repo pack.
/// Without that folder, the pinned licensed engine pack is fetched and
/// merged in its place.
#[cfg(feature = "world")]
pub fn stage_effects_pack(assets_dir: &Path, out: &Path) -> Result<()> {
    let folder = crate::pack_sounds::resolve_licensed_overlay(assets_dir, None);
    let licensed = if folder.is_dir() {
        None
    } else {
        ensure_licensed_sounds_pack(assets_dir)?
    };
    crate::pack_sounds::stage_effects_pack(assets_dir, out, None, licensed.as_deref())?;
    verify_staged_engine_pack(out).map_err(|e| {
        if !folder.is_dir() && licensed.is_none() {
            format!("{e}. {}", NO_LICENSED_PIN)
        } else {
            e
        }
    })
}

/// Added to the guard's refusal when there is neither a folder nor a pin.
#[cfg(feature = "world")]
const NO_LICENSED_PIN: &str = "No licensed engine pack is pinned either \
     (DEFAULT_LICENSED_SOUNDS_URL in crates/ff-tools/src/release/packs.rs, or \
     FREIGHT_FATE_LICENSED_PACK_URL and FREIGHT_FATE_LICENSED_PACK_SHA256)";

#[cfg(not(feature = "world"))]
pub fn stage_effects_pack(assets_dir: &Path, out: &Path) -> Result<()> {
    util::copy_file(&assets_dir.join("sounds.pak"), out)?;
    verify_staged_engine_pack(out)
}

fn allow_unlicensed_release() -> bool {
    std::env::var("FREIGHT_FATE_ALLOW_UNLICENSED_RELEASE").is_ok_and(|value| value.trim() == "1")
}

#[cfg(feature = "world")]
fn verify_staged_engine_pack(path: &Path) -> Result<()> {
    if allow_unlicensed_release() {
        return Ok(());
    }
    crate::pack_sounds::verify_licensed_engine_in_pack(path)
}

#[cfg(not(feature = "world"))]
fn verify_staged_engine_pack(_path: &Path) -> Result<()> {
    if allow_unlicensed_release() {
        Ok(())
    } else {
        Err(
            "this build of ff-tools has no `world` feature, so it cannot verify the licensed \
             stereo engine in the staged sound pack"
                .into(),
        )
    }
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
    if !allow_unlicensed_release() {
        let missing = missing_licensed_engine_entries(&names);
        if !missing.is_empty() {
            return Err(format!(
                "Packaged sound pack is missing licensed stereo cab engine cuts ({}), so \
                 the build would fall back silently to the 1.9 mono engine; provide \
                 assets/sounds-licensed (or FREIGHT_FATE_LICENSED_SOUNDS) when staging \
                 (or set FREIGHT_FATE_ALLOW_UNLICENSED_RELEASE=1 for an open test build)",
                missing.join(", ")
            ));
        }
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

    #[cfg(feature = "world")]
    #[test]
    fn staged_effects_pack_rejects_missing_licensed_engine() {
        let dir = tempfile::tempdir().unwrap();
        let sounds = dir.path().join("sounds");
        std::fs::create_dir_all(&sounds).unwrap();
        std::fs::write(sounds.join("horn.ogg"), b"horn").unwrap();
        let pack = dir.path().join("sounds.pak");
        ff_core::assets_pack::write_pack(&sounds, &pack, None, None).unwrap();

        let err = verify_staged_engine_pack(&pack).unwrap_err();
        assert!(
            err.contains("engine_stereo/idle.wav"),
            "expected missing stereo engine error, got: {err}"
        );
    }

    #[cfg(feature = "world")]
    #[test]
    fn verify_sound_packs_requires_licensed_stereo_engine() {
        let dir = tempfile::tempdir().unwrap();
        let build = dir.path().join("build");
        let pkg = build.join("freight_fate");
        let src = dir.path().join("src");
        std::fs::create_dir_all(src.join("engine_classic")).unwrap();
        std::fs::create_dir_all(src.join("music")).unwrap();
        std::fs::create_dir_all(src.join("c3k")).unwrap();
        std::fs::write(src.join("engine_classic/idle.ogg"), b"idle").unwrap();
        std::fs::write(src.join("music/track.ogg"), b"music").unwrap();
        std::fs::write(src.join("c3k/clip.ogg"), b"c3k").unwrap();

        ff_core::assets_pack::write_pack(
            &src,
            &pkg.join("sounds.pak"),
            None,
            Some(&|n| !n.starts_with("music/") && !n.starts_with("c3k/")),
        )
        .unwrap();
        ff_core::assets_pack::write_pack(
            &src,
            &pkg.join("music.pak"),
            None,
            Some(&|n| n.starts_with("music/")),
        )
        .unwrap();
        ff_core::assets_pack::write_pack(
            &src,
            &pkg.join("channel3000.pak"),
            None,
            Some(&|n| n.starts_with("c3k/")),
        )
        .unwrap();

        let err = verify_sound_packs(&build).unwrap_err();
        assert!(
            err.contains("engine_stereo/idle.wav"),
            "expected missing stereo engine error, got: {err}"
        );

        let overlay = dir.path().join("overlay");
        std::fs::create_dir_all(overlay.join("engine_stereo")).unwrap();
        for entry in REQUIRED_LICENSED_ENGINE_ENTRIES {
            std::fs::write(overlay.join(entry), b"wav").unwrap();
        }
        ff_core::assets_pack::write_pack_with_base(
            Some(&pkg.join("sounds.pak")),
            &src,
            &pkg.join("sounds.pak"),
            Some(&overlay),
            Some(&|n| !n.starts_with("music/") && !n.starts_with("c3k/")),
        )
        .unwrap();
        verify_sound_packs(&build).unwrap();
    }

    #[cfg(feature = "world")]
    #[test]
    fn an_empty_pin_means_no_licensed_pack() {
        if std::env::var_os("FREIGHT_FATE_LICENSED_PACK_URL").is_some()
            || std::env::var_os("FREIGHT_FATE_LICENSED_PACK_SHA256").is_some()
        {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(ensure_licensed_sounds_pack(dir.path()).unwrap(), None);
        assert!(!dir.path().join(LICENSED_PACK_FILE).exists());
    }

    #[cfg(feature = "world")]
    #[test]
    fn unpinned_staging_without_a_folder_names_the_pin() {
        if std::env::var_os("FREIGHT_FATE_LICENSED_PACK_URL").is_some()
            || std::env::var_os("FREIGHT_FATE_LICENSED_SOUNDS").is_some()
            || allow_unlicensed_release()
        {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let assets = dir.path().join("assets");
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("horn.ogg"), b"horn").unwrap();
        ff_core::assets_pack::write_pack(&src, &assets.join("sounds.pak"), None, None).unwrap();
        let err = stage_effects_pack(&assets, &dir.path().join("staged.pak")).unwrap_err();
        assert!(err.contains("engine_stereo/idle.wav"), "{err}");
        assert!(err.contains("No licensed engine pack is pinned"), "{err}");
    }
}
