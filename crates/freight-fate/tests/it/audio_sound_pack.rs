//! Port of the `audio._asset_bytes` / `_asset_path` / `verify_sound_assets`
//! tests of `tests/test_sound_pack.py`: pack-then-loose resolution, the
//! licensed overlay, and the engine-ring rule. The pack format, split and
//! loader tests live with `ff_core::assets_pack`.
//!
//! The Python tests marked `needs_loose_tree` ran only on a machine with the
//! loose sound tree; here a fixture tree stands in, passed through the
//! `_from` / `_in` variants of the lookup, so the rules are pinned on any
//! checkout.
//!
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ff_core::assets_pack::{write_pack, CombinedPack, SoundPack};
use freight_fate::audio::{
    asset_bytes, asset_bytes_from, asset_path_in, verify_sound_assets, MUSIC_EXTENSIONS,
    SFX_EXTENSIONS,
};

use crate::audio_support::shipped_sounds;

fn write_fixture_sounds(tmp: &Path) -> PathBuf {
    let sounds = tmp.join("sounds");
    std::fs::create_dir_all(sounds.join("ui")).unwrap();
    std::fs::create_dir_all(sounds.join("music")).unwrap();
    std::fs::write(
        sounds.join("ui").join("menu_select.ogg"),
        b"fake ogg for menu select",
    )
    .unwrap();
    std::fs::write(
        sounds.join("music").join("open_road.wav"),
        b"fake wav for open road",
    )
    .unwrap();
    sounds
}

fn pack_from(sounds: &Path, out: &Path) -> CombinedPack {
    let path = write_pack(sounds, out, None, None).unwrap();
    CombinedPack::new(Some(Arc::new(SoundPack::open(&path).unwrap())), None)
}

#[test]
fn test_asset_path_prefers_licensed_overlay() {
    let tmp = tempfile::tempdir().unwrap();
    let base = write_fixture_sounds(tmp.path());
    let overlay = tmp.path().join("licensed");
    std::fs::create_dir_all(overlay.join("ui")).unwrap();
    // Overlay wins even across the extension preference order: its .wav
    // beats the base tree's .ogg for the same key.
    std::fs::write(overlay.join("ui").join("menu_select.wav"), b"licensed wav").unwrap();
    let roots = [overlay.clone(), base.clone()];
    let found = asset_path_in(&roots, "ui/menu_select", SFX_EXTENSIONS);
    assert_eq!(found, Some(overlay.join("ui").join("menu_select.wav")));
    // Keys the overlay does not carry still resolve from the base tree.
    assert!(asset_path_in(&roots, "music/open_road", SFX_EXTENSIONS).is_some());
}

#[test]
fn test_asset_bytes_prefers_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let sounds = write_fixture_sounds(tmp.path());
    let pack = pack_from(&sounds, &tmp.path().join("sounds.pak"));
    // The loose root carries a different byte string for the same key.
    let loose = tmp.path().join("loose");
    std::fs::create_dir_all(loose.join("ui")).unwrap();
    std::fs::write(loose.join("ui").join("menu_select.ogg"), b"loose copy").unwrap();
    let found = asset_bytes_from(Some(&pack), &[loose], "ui/menu_select", SFX_EXTENSIONS).unwrap();
    assert_eq!(
        (&found.0[..], found.1.as_str()),
        (&b"fake ogg for menu select"[..], "ogg")
    );
}

#[test]
fn test_asset_bytes_reads_loose_files_without_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let sounds = write_fixture_sounds(tmp.path());
    let found = asset_bytes_from(
        None,
        std::slice::from_ref(&sounds),
        "ui/menu_select",
        SFX_EXTENSIONS,
    )
    .unwrap();
    assert_eq!(
        &found.0[..],
        &std::fs::read(sounds.join("ui").join("menu_select.ogg")).unwrap()[..]
    );
    assert_eq!(found.1, "ogg");
    assert!(asset_bytes_from(None, &[sounds], "ui/nothing", SFX_EXTENSIONS).is_none());
}

#[test]
fn test_verify_sound_assets_passes_in_source_checkout() {
    // "In a source checkout" means one that HAS the sounds: the check
    // exists to prove a frozen build can read its pack, and a checkout
    // holding only a pointer to that pack has nothing to prove it with.
    if !shipped_sounds() {
        return;
    }
    verify_sound_assets().unwrap();
    // And the real lookup agrees with the packed canonical sound.
    assert!(asset_bytes("ui/menu_select", SFX_EXTENSIONS).is_some());
}

#[test]
fn test_default_pack_has_no_legacy_engine_samples() {
    if !shipped_sounds() {
        return;
    }
    for key in [
        "engine/idle",
        "engine/low",
        "engine/mid",
        "engine/midhigh",
        "engine/high",
    ] {
        assert!(
            asset_bytes(key, SFX_EXTENSIONS).is_none(),
            "{key} remained in the default pack"
        );
    }
}

#[test]
fn test_older_pack_still_serves_the_keys_it_has() {
    // munchkinbear's case: an older pack alongside a current checkout.
    // Keys the old pack carries play from it; keys added since come off
    // the loose tree.
    let tmp = tempfile::tempdir().unwrap();
    let old = write_fixture_sounds(tmp.path());
    let pack = pack_from(&old, &tmp.path().join("sounds.pak"));
    let loose = tmp.path().join("loose");
    std::fs::create_dir_all(loose.join("vehicle")).unwrap();
    std::fs::write(loose.join("vehicle").join("edge_strip.wav"), b"newer cue").unwrap();
    let roots = [loose];
    let found = asset_bytes_from(Some(&pack), &roots, "ui/menu_select", SFX_EXTENSIONS).unwrap();
    assert_eq!(
        (&found.0[..], found.1.as_str()),
        (&b"fake ogg for menu select"[..], "ogg")
    );
    let newer = asset_bytes_from(Some(&pack), &roots, "vehicle/edge_strip", SFX_EXTENSIONS);
    assert!(newer.is_some()); // not in the old pack; comes from the checkout
}

#[test]
fn test_missing_music_pack_falls_back_to_loose_music_files() {
    // End to end through the lookup: a music key falls back to the loose
    // tree while an unrelated sounds.pak key still comes off the pack --
    // the two packs fail independently.
    let tmp = tempfile::tempdir().unwrap();
    let sounds_src = tmp.path().join("sounds_src");
    std::fs::create_dir_all(sounds_src.join("ui")).unwrap();
    std::fs::write(
        sounds_src.join("ui").join("menu_select.ogg"),
        b"fake ogg for menu select",
    )
    .unwrap();
    let pack = pack_from(&sounds_src, &tmp.path().join("sounds.pak")); // no music side
    let loose = tmp.path().join("loose");
    std::fs::create_dir_all(loose.join("music")).unwrap();
    std::fs::write(
        loose.join("music").join("drive_always_around.wav"),
        b"loose music",
    )
    .unwrap();
    let roots = [loose.clone()];
    let (data, ext) = asset_bytes_from(
        Some(&pack),
        &roots,
        "music/drive_always_around",
        MUSIC_EXTENSIONS,
    )
    .unwrap();
    assert_eq!(
        &data[..],
        &std::fs::read(
            loose
                .join("music")
                .join(format!("drive_always_around.{ext}"))
        )
        .unwrap()[..]
    );
    let found = asset_bytes_from(Some(&pack), &roots, "ui/menu_select", SFX_EXTENSIONS).unwrap();
    assert_eq!(
        (&found.0[..], found.1.as_str()),
        (&b"fake ogg for menu select"[..], "ogg")
    );
}
