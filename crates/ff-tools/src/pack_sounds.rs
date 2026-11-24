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
pub fn stage_effects_pack(
    assets_dir: &Path,
    out: &Path,
    overlay_override: Option<&Path>,
) -> Result<()> {
    let base_pack = assets_dir.join("sounds.pak");
    let sounds = assets_dir.join("sounds");
    let overlay = resolve_licensed_overlay(assets_dir, overlay_override);
    pack(
        base_pack.is_file().then_some(base_pack.as_path()),
        &sounds,
        &overlay,
        out,
        false,
    )
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
            values: &["out", "music-out", "overlay"],
            flags: &["require-licensed"],
        },
    )
    .map_err(|e| {
        format!(
            "{e}\nusage: pack-sounds [--out FILE] [--music-out FILE] [--overlay DIR] [--require-licensed]"
        )
    })?;
    let root = util::repo_root();
    let assets = root.join("assets");
    let sounds = assets.join("sounds");
    let overlay_arg = args.path("overlay");
    let overlay = resolve_licensed_overlay(&assets, overlay_arg.as_deref());
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
        stage_effects_pack(&assets, &staged, None).unwrap();
        verify_licensed_engine_in_pack(&staged).unwrap();
        let names = SoundPack::open(&staged).unwrap().names().to_vec();
        assert!(names.iter().any(|n| n == "vehicle/horn.ogg"));
        assert!(!names.iter().any(|n| n == "engine/idle.wav"));
        for entry in REQUIRED_LICENSED_ENGINE_ENTRIES {
            assert!(names.iter().any(|n| n == entry));
        }
    }
}
