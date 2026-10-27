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

use std::path::Path;

use ff_core::assets_pack::{write_pack, SoundPack};

use crate::cli::{Args, Spec};
use crate::util::{self, Result};

fn pack(sounds: &Path, overlay: &Path, out: &Path, music: bool) -> Result<()> {
    let include = move |name: &str| name.starts_with("music/") == music;
    write_pack(sounds, out, Some(overlay), Some(&include))
        .map_err(|e| format!("could not write {}: {e}", out.display()))?;
    Ok(())
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["out", "music-out"],
            flags: &[],
        },
    )
    .map_err(|e| format!("{e}\nusage: pack-sounds [--out FILE] [--music-out FILE]"))?;
    let root = util::repo_root();
    let sounds = root.join("assets").join("sounds");
    let overlay = root.join("assets").join("sounds-licensed");
    let out = args
        .path("out")
        .unwrap_or_else(|| root.join("build").join("sounds.pak"));
    let music_out = args
        .path("music-out")
        .unwrap_or_else(|| root.join("build").join("music.pak"));
    pack(&sounds, &overlay, &out, false)?;
    pack(&sounds, &overlay, &music_out, true)?;
    for (label, path) in [("sound", &out), ("music", &music_out)] {
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
        pack(&sounds, &dir.path().join("none"), &fx, false).unwrap();
        pack(&sounds, &dir.path().join("none"), &music, true).unwrap();
        assert_eq!(SoundPack::open(&fx).unwrap().names(), ["horn.ogg"]);
        assert_eq!(SoundPack::open(&music).unwrap().names(), ["music/a.ogg"]);
    }
}
