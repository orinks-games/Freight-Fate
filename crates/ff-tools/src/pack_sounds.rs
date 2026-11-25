//! `pack-sounds`: pack the sound assets into the masked files a release
//! ships.
//!
//! The shipped game carries `freight_fate/sounds.pak` and
//! `freight_fate/music.pak` instead of a browsable sounds folder. The
//! `music/` subtree packs separately so the small effects payload does not
//! travel with the much larger music library on every change. Source
//! checkouts keep the loose, editable `assets/sounds` tree; the audio engine
//! only reads a pack when it exists. The licensed overlay
//! (`assets/sounds-licensed`, never committed) is packed over the committed
//! tree when present.
//!
//! A machine without that folder (every CI builder) gets the licensed cuts
//! from a licensed-only pack instead: `pack-sounds --licensed-out FILE`
//! writes one from the overlay on the owner's machine, it is hosted at the
//! pinned address in `release/packs.rs` (`DEFAULT_LICENSED_SOUNDS_URL`,
//! checked by SHA-256 like `music.pak`), and staging downloads it to the
//! gitignored `assets/sounds-licensed.pak` and merges it exactly as it
//! would the folder. A local folder always wins over the pinned pack.

use std::path::{Path, PathBuf};

use ff_core::assets_pack::{write_pack_with_base, SoundPack};

use crate::cli::{Args, Spec};
use crate::util::{self, Result};

const LEGACY_ENGINE_STEMS: &[&str] = &[
    "engine/idle",
    "engine/low",
    "engine/mid",
    "engine/midhigh",
    "engine/high",
];

/// The calibrated stereo cab engine loops and synchronized starter required
/// in every licensed release `sounds.pak`.
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

/// Fail loudly if `path` is missing any licensed stereo cab engine entry.
pub fn verify_licensed_engine_in_pack(path: &Path) -> Result<()> {
    let pack = SoundPack::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let missing = missing_licensed_engine_entries(&pack.names());
    if !missing.is_empty() {
        return Err(format!(
            "Sound pack {} is missing required licensed stereo cab engine cuts ({}); \
             provide assets/sounds-licensed (or FREIGHT_FATE_LICENSED_SOUNDS) and rebuild \
             with `cargo tools pack-sounds --require-licensed`",
            path.display(),
            missing.join(", ")
        ));
    }
    Ok(())
}

/// Resolve the licensed overlay directory: explicit path, then
/// `FREIGHT_FATE_LICENSED_SOUNDS`, then `<assets_dir>/sounds-licensed`.
pub fn resolve_licensed_overlay(assets_dir: &Path, explicit: Option<&Path>) -> PathBuf {
    if let Some(path) = explicit {
        return path.to_path_buf();
    }
    if let Ok(env_dir) = std::env::var("FREIGHT_FATE_LICENSED_SOUNDS") {
        if !env_dir.trim().is_empty() {
            return PathBuf::from(env_dir);
        }
    }
    assets_dir.join("sounds-licensed")
}

/// Stage a single runtime `sounds.pak` by merging the base `assets/sounds.pak`,
/// any committed loose files under `assets/sounds`, and the private licensed
/// overlay when present (never modifying the tracked `assets/sounds.pak`).
///
/// `licensed_pack` is a licensed-only pack (see `--licensed-out`) used in
/// place of the overlay folder when that folder does not exist; its entries
/// merge with the same precedence the folder's would.
pub fn stage_effects_pack(
    assets_dir: &Path,
    out: &Path,
    overlay_override: Option<&Path>,
    licensed_pack: Option<&Path>,
) -> Result<()> {
    let base_pack = assets_dir.join("sounds.pak");
    let sounds = assets_dir.join("sounds");
    let mut overlay = resolve_licensed_overlay(assets_dir, overlay_override);
    // Held until the pack is written; dropping it deletes the extracted cuts.
    let mut _extracted = None;
    if !overlay.is_dir() {
        if let Some(licensed) = licensed_pack {
            let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
            extract_pack(licensed, dir.path())?;
            overlay = dir.path().to_path_buf();
            _extracted = Some(dir);
        }
    }
    pack(
        base_pack.is_file().then_some(base_pack.as_path()),
        &sounds,
        &overlay,
        out,
        false,
    )
}

/// Write every entry of `pack` under `dir` as a loose file, so it can be
/// merged as an overlay folder.
fn extract_pack(pack: &Path, dir: &Path) -> Result<()> {
    let opened = SoundPack::open(pack).map_err(|e| format!("{}: {e}", pack.display()))?;
    for name in opened.names() {
        let relative = Path::new(&name);
        if relative.is_absolute()
            || relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(format!(
                "{} holds an entry outside the pack: {name}",
                pack.display()
            ));
        }
        let bytes = opened
            .read(&name)
            .ok_or_else(|| format!("{}: cannot read {name}", pack.display()))?;
        let dest = dir.join(relative);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&dest, bytes).map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    Ok(())
}

/// Write the licensed-only pack: every non-music entry of the overlay, and
/// nothing from the committed tree. Fails if a required engine cut is absent.
fn write_licensed_pack(overlay: &Path, out: &Path) -> Result<()> {
    if !overlay.is_dir() {
        return Err(format!(
            "Licensed sound folder was not found: {}",
            overlay.display()
        ));
    }
    let include = |name: &str| !name.starts_with("music/");
    write_pack_with_base(None, overlay, out, None, Some(&include))
        .map_err(|e| format!("could not write {}: {e}", out.display()))?;
    verify_licensed_engine_in_pack(out)
}

fn pack(
    base_pack: Option<&Path>,
    sounds: &Path,
    overlay: &Path,
    out: &Path,
    music: bool,
) -> Result<()> {
    let include = move |name: &str| {
        name.starts_with("music/") == music
            && (music
                || !LEGACY_ENGINE_STEMS.iter().any(|stem| {
                    name.strip_prefix(stem)
                        .is_some_and(|rest| rest.starts_with('.'))
                }))
    };
    write_pack_with_base(base_pack, sounds, out, Some(overlay), Some(&include))
        .map_err(|e| format!("could not write {}: {e}", out.display()))?;
    Ok(())
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["out", "music-out", "overlay", "licensed-out"],
            flags: &["require-licensed"],
        },
    )
    .map_err(|e| {
        format!(
            "{e}\nusage: pack-sounds [--out FILE] [--music-out FILE] [--overlay DIR] [--require-licensed]\n       pack-sounds --licensed-out FILE [--overlay DIR]"
        )
    })?;
    let root = util::repo_root();
    let assets = root.join("assets");
    let sounds = assets.join("sounds");
    let overlay_arg = args.path("overlay");
    let overlay = resolve_licensed_overlay(&assets, overlay_arg.as_deref());
    if let Some(licensed_out) = args.path("licensed-out") {
        write_licensed_pack(&overlay, &licensed_out)?;
        let entries = SoundPack::open(&licensed_out)
            .map_err(|e| format!("{}: {e}", licensed_out.display()))?
            .names()
            .len();
        let size = licensed_out.metadata().map(|m| m.len()).unwrap_or(0) as f64 / 1e6;
        println!(
            "Packed {entries} licensed asset(s) into {} ({size:.1} MB)\nSHA-256: {}",
            licensed_out.display(),
            util::file_sha256(&licensed_out)?
        );
        return Ok(0);
    }
    let base_fx = assets.join("sounds.pak");
    let base_music = assets.join("music.pak");
    let out = args
        .path("out")
        .unwrap_or_else(|| root.join("build").join("sounds.pak"));
    let music_out = args
        .path("music-out")
        .unwrap_or_else(|| root.join("build").join("music.pak"));
    pack(
        base_fx.is_file().then_some(base_fx.as_path()),
        &sounds,
        &overlay,
        &out,
        false,
    )?;
    if args.flag("require-licensed") {
        verify_licensed_engine_in_pack(&out)?;
    }
    let has_music_base = base_music.is_file();
    if has_music_base || sounds.join("music").is_dir() || overlay.join("music").is_dir() {
        pack(
            has_music_base.then_some(base_music.as_path()),
            &sounds,
            &overlay,
            &music_out,
            true,
        )?;
    }
    let mut outputs = vec![("sound", &out)];
    if music_out.is_file() {
        outputs.push(("music", &music_out));
    }
    for (label, path) in outputs {
        let entries = SoundPack::open(path)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .names()
            .len();
        let size = path.metadata().map(|m| m.len()).unwrap_or(0) as f64 / 1e6;
        println!(
            "Packed {entries} {label} asset(s) into {} ({size:.1} MB)",
            path.display()
        );
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn music_and_effects_land_in_separate_packs() {
        let dir = tempfile::tempdir().unwrap();
        let sounds = dir.path().join("sounds");
        std::fs::create_dir_all(sounds.join("music")).unwrap();
        std::fs::write(sounds.join("horn.ogg"), b"h").unwrap();
        std::fs::write(sounds.join("music").join("a.ogg"), b"m").unwrap();
        let (fx, music) = (dir.path().join("s.pak"), dir.path().join("m.pak"));
        pack(None, &sounds, &dir.path().join("none"), &fx, false).unwrap();
        pack(None, &sounds, &dir.path().join("none"), &music, true).unwrap();
        assert_eq!(SoundPack::open(&fx).unwrap().names(), ["horn.ogg"]);
        assert_eq!(SoundPack::open(&music).unwrap().names(), ["music/a.ogg"]);
    }

    #[test]
    fn stage_effects_pack_merges_licensed_overlay_and_verifies_stereo_engine() {
        let dir = tempfile::tempdir().unwrap();
        let assets = dir.path().join("assets");
        let sounds = assets.join("sounds");
        let overlay = assets.join("sounds-licensed");
        std::fs::create_dir_all(sounds.join("vehicle")).unwrap();
        std::fs::create_dir_all(sounds.join("engine")).unwrap();
        std::fs::create_dir_all(overlay.join("engine_stereo")).unwrap();
        std::fs::write(sounds.join("vehicle/horn.ogg"), b"base-horn").unwrap();
        std::fs::write(sounds.join("engine/idle.wav"), b"legacy-engine").unwrap();
        let base_pack = assets.join("sounds.pak");
        pack(None, &sounds, &dir.path().join("none"), &base_pack, false).unwrap();

        // Without the licensed overlay, verification refuses the pack.
        assert!(verify_licensed_engine_in_pack(&base_pack).is_err());

        for entry in REQUIRED_LICENSED_ENGINE_ENTRIES {
            std::fs::write(overlay.join(entry), b"wav").unwrap();
        }
        let staged = dir.path().join("staged_sounds.pak");
        stage_effects_pack(&assets, &staged, None, None).unwrap();
        verify_licensed_engine_in_pack(&staged).unwrap();
        let names = SoundPack::open(&staged).unwrap().names().to_vec();
        assert!(names.iter().any(|n| n == "vehicle/horn.ogg"));
        assert!(!names.iter().any(|n| n == "engine/idle.wav"));
        for entry in REQUIRED_LICENSED_ENGINE_ENTRIES {
            assert!(names.iter().any(|n| n == entry));
        }
    }

    /// An overlay folder holding the required cuts, a music file, and a
    /// replacement horn.
    fn licensed_folder(root: &Path, marker: &[u8]) -> PathBuf {
        let overlay = root.join("licensed");
        std::fs::create_dir_all(overlay.join("engine_stereo")).unwrap();
        std::fs::create_dir_all(overlay.join("music")).unwrap();
        for entry in REQUIRED_LICENSED_ENGINE_ENTRIES {
            std::fs::write(overlay.join(entry), marker).unwrap();
        }
        std::fs::write(overlay.join("music/song.ogg"), b"song").unwrap();
        overlay
    }

    /// An assets folder with a base pack holding a horn and a legacy engine.
    fn base_assets(root: &Path) -> PathBuf {
        let assets = root.join("assets");
        let sounds = root.join("base-src");
        std::fs::create_dir_all(sounds.join("vehicle")).unwrap();
        std::fs::create_dir_all(sounds.join("engine")).unwrap();
        std::fs::write(sounds.join("vehicle/horn.ogg"), b"base-horn").unwrap();
        std::fs::write(sounds.join("engine/idle.wav"), b"legacy-engine").unwrap();
        pack(
            None,
            &sounds,
            &root.join("none"),
            &assets.join("sounds.pak"),
            false,
        )
        .unwrap();
        assets
    }

    #[test]
    fn licensed_only_pack_holds_just_the_overlay_without_music() {
        let dir = tempfile::tempdir().unwrap();
        let overlay = licensed_folder(dir.path(), b"cut");
        let out = dir.path().join("sounds-licensed.pak");
        write_licensed_pack(&overlay, &out).unwrap();
        let pack = SoundPack::open(&out).unwrap();
        let mut expected: Vec<String> = REQUIRED_LICENSED_ENGINE_ENTRIES
            .iter()
            .map(|e| e.to_string())
            .collect();
        expected.sort();
        assert_eq!(pack.names(), expected);
        assert_eq!(pack.read("engine_stereo/idle.wav").unwrap(), b"cut");
    }

    #[test]
    fn licensed_only_pack_refuses_a_missing_engine_cut() {
        let dir = tempfile::tempdir().unwrap();
        let overlay = licensed_folder(dir.path(), b"cut");
        std::fs::remove_file(overlay.join("engine_stereo/midhigh.wav")).unwrap();
        let err = write_licensed_pack(&overlay, &dir.path().join("l.pak")).unwrap_err();
        assert!(err.contains("engine_stereo/midhigh.wav"), "{err}");
        let err =
            write_licensed_pack(&dir.path().join("absent"), &dir.path().join("l.pak")).unwrap_err();
        assert!(err.contains("not found"), "{err}");
    }

    #[test]
    fn staging_merges_a_licensed_pack_when_no_folder_exists() {
        let dir = tempfile::tempdir().unwrap();
        let assets = base_assets(dir.path());
        let overlay = licensed_folder(dir.path(), b"from-pack");
        let licensed = dir.path().join("sounds-licensed.pak");
        write_licensed_pack(&overlay, &licensed).unwrap();

        let staged = dir.path().join("staged.pak");
        let no_folder = dir.path().join("no-folder");
        stage_effects_pack(&assets, &staged, Some(&no_folder), Some(&licensed)).unwrap();
        verify_licensed_engine_in_pack(&staged).unwrap();
        let pack = SoundPack::open(&staged).unwrap();
        assert_eq!(pack.read("vehicle/horn.ogg").unwrap(), b"base-horn");
        assert_eq!(pack.read("engine_stereo/high.wav").unwrap(), b"from-pack");
        assert!(!pack.has("engine/idle.wav"));
        assert!(!pack.names().iter().any(|n| n.starts_with("music/")));
    }

    #[test]
    fn a_local_folder_beats_the_licensed_pack() {
        let dir = tempfile::tempdir().unwrap();
        let assets = base_assets(dir.path());
        let pack_src = tempfile::tempdir().unwrap();
        let licensed = dir.path().join("sounds-licensed.pak");
        write_licensed_pack(&licensed_folder(pack_src.path(), b"from-pack"), &licensed).unwrap();
        let folder = licensed_folder(dir.path(), b"from-folder");

        let staged = dir.path().join("staged.pak");
        stage_effects_pack(&assets, &staged, Some(&folder), Some(&licensed)).unwrap();
        let pack = SoundPack::open(&staged).unwrap();
        assert_eq!(pack.read("engine_stereo/idle.wav").unwrap(), b"from-folder");
    }
}
