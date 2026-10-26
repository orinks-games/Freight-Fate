//! Where sounds come from: the generated-sound registry, the shipped packs,
//! the licensed overlay and the loose tree -- in that order -- plus the
//! clip-length and cab-sealed caches built on top of the lookup.
//!
//! The module-level half of `freight_fate/audio.py` (`_asset_path`,
//! `_asset_bytes`, `_playback_bytes`, `asset_length_s`,
//! `verify_sound_assets`). The lookup is also offered with the pack and the
//! roots injected (`asset_bytes_from`, `asset_path_in`) so the resolution
//! rules can be pinned against fixture packs and trees without touching the
//! process-wide defaults.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ff_core::assets_pack::{self, generated_sound, generated_sound_version, CombinedPack};
use ff_core::cab_filter;
use ff_core::data::data_resources::data_root;
use once_cell::sync::Lazy;

use super::{engine_band_keys, is_engine_band_key, AudioError};

/// The bytes behind a sound key and the extension they were found under.
pub type AssetBytes = (Arc<[u8]>, String);

/// Extension preference for effects and loops.
pub const SFX_EXTENSIONS: &[&str] = &["ogg", "wav"];
/// Extension preference for music. Music ships as Opus
/// (`tools/encode_music_opus.py`): far smaller for background beds at the
/// same perceived quality. Ogg stays in the list so a partial migration and
/// the effects tree, which are still Vorbis, keep resolving. WAV, MP3 and
/// FLAC decode too. Tracker modules still resolve, so a key that names one
/// is found and refused (see [`MODULE_EXTENSIONS`]) rather than missing.
pub const MUSIC_EXTENSIONS: &[&str] = &[
    "opus", "ogg", "wav", "mp3", "flac", "it", "xm", "s3m", "mod", "mo3",
];
/// Tracker modules (as made in OpenMPT). BASS played them; Hearken has no
/// tracker player, so the music channel refuses them and a playlist skips
/// the entry.
pub const MODULE_EXTENSIONS: &[&str] = &["it", "xm", "s3m", "mod", "mo3"];

/// The parent of the data tree: the repo root in a checkout (`data/`,
/// `assets/sounds/`, `assets/lib/`), `<exe dir>/freight_fate/` when packaged
/// (`data/`, `assets/sounds/`, `lib/`).
pub fn package_root() -> PathBuf {
    package_root_for(data_root())
}

fn package_root_for(data_root: &Path) -> PathBuf {
    data_root
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("freight_fate"))
}

/// The committed sound tree (`ASSETS` in Python).
pub fn assets_dir() -> PathBuf {
    package_root().join("assets").join("sounds")
}

/// Licensed sound-library overlay (gitignored, never committed). A machine
/// that owns the purchased libraries drops encoded assets here under the
/// same keys; they take precedence over the committed tree, so a clean
/// clone still runs on the synthesized fallbacks. Release builds bake the
/// overlay into sounds.pak.
pub fn assets_licensed_dir() -> PathBuf {
    package_root().join("assets").join("sounds-licensed")
}

/// The loose-file roots in lookup order: the licensed overlay, then the
/// committed tree.
pub fn asset_roots() -> [PathBuf; 2] {
    [assets_licensed_dir(), assets_dir()]
}

/// Loose-file lookup over the given roots; source checkouts and asset
/// tooling only.
pub fn asset_path_in(roots: &[PathBuf], key: &str, extensions: &[&str]) -> Option<PathBuf> {
    for root in roots {
        for ext in extensions {
            let path = root.join(format!("{key}.{ext}"));
            if path.exists() {
                return Some(path);
            }
        }
    }
    None
}

/// Loose-file lookup over the default roots.
pub fn asset_path(key: &str, extensions: &[&str]) -> Option<PathBuf> {
    asset_path_in(&asset_roots(), key, extensions)
}

/// Whether `pack` holds every engine band cut. Membership only, so nothing
/// is decompressed.
pub fn pack_carries_whole_ring(pack: &CombinedPack) -> bool {
    engine_band_keys().iter().all(|key| {
        SFX_EXTENSIONS
            .iter()
            .any(|ext| pack.has(&format!("{key}.{ext}")))
    })
}

/// Bytes and extension for a sound key, from `pack` or the loose `roots`.
///
/// The engine bands are the one exception to pack-then-loose: they
/// crossfade into each other, so they have to come from one recording. A
/// pack that predates the checkout beside it would otherwise serve four
/// bands from the pack and the fifth off disk, blending two different
/// engines. Unless the pack carries the whole ring, the ring reads from the
/// loose tree.
pub fn asset_bytes_from(
    pack: Option<&CombinedPack>,
    roots: &[PathBuf],
    key: &str,
    extensions: &[&str],
) -> Option<AssetBytes> {
    if let Some((data, ext)) = generated_sound(key) {
        return Some((Arc::from(data.as_slice()), ext));
    }
    let pack = pack.filter(|pack| !is_engine_band_key(key) || pack_carries_whole_ring(pack));
    if let Some(pack) = pack {
        for ext in extensions {
            if let Some(data) = pack.read(&format!("{key}.{ext}")) {
                return Some((Arc::from(data), (*ext).to_string()));
            }
        }
    }
    let path = asset_path_in(roots, key, extensions)?;
    match std::fs::read(&path) {
        Ok(data) => {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_string();
            Some((Arc::from(data), ext))
        }
        Err(err) => {
            log::warn!("Unreadable sound file: {} ({err})", path.display());
            None
        }
    }
}

/// Bytes and extension for a sound key, from the shipped pack or loose
/// files.
///
/// Frozen builds carry the sounds packed into `sounds.pak` (see
/// `assets_pack`); source checkouts read the editable `assets/sounds` tree.
/// Synthesized cues published through `register_generated_sound` win over
/// both.
pub fn asset_bytes(key: &str, extensions: &[&str]) -> Option<AssetBytes> {
    let pack = assets_pack::open_default();
    asset_bytes_from(pack.as_deref(), &asset_roots(), key, extensions)
}

/// A cache whose entries each remember their key's generated-sound version:
/// registering or dropping that key invalidates its entry alone, because
/// anything measured or rendered from the old bytes was measuring nothing
/// (the Python `register_generated_sound` popped the key from `_LENGTHS`
/// and `_CAB_SEALED`). A synthesized piece coming and going never costs an
/// engine band its sealed cut.
struct VersionedCache<T> {
    map: HashMap<String, (u64, T)>,
}

impl<T> VersionedCache<T> {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    fn get(&self, key: &str) -> Option<&T> {
        self.map
            .get(key)
            .filter(|(version, _)| *version == generated_sound_version(key))
            .map(|(_, value)| value)
    }

    fn insert(&mut self, key: &str, value: T) {
        self.map
            .insert(key.to_string(), (generated_sound_version(key), value));
    }
}

/// Engine band cuts with the sealed-cab transfer applied, by key. The
/// transfer is deterministic and the cuts change only on a repack, so
/// sealing each cut once per process is enough for every engine start after
/// the first.
static CAB_SEALED: Lazy<Mutex<VersionedCache<AssetBytes>>> =
    Lazy::new(|| Mutex::new(VersionedCache::new()));

/// Measured playing times, by sound key. See [`asset_length_s`].
static LENGTHS: Lazy<Mutex<VersionedCache<f64>>> = Lazy::new(|| Mutex::new(VersionedCache::new()));

/// Bytes for a sound as the player should HEAR it.
///
/// The engine band cuts pass through the sealed-cab transfer
/// (`cab_filter`, owner's ear 2026-08-13): the recorded voice reads as a
/// truck heard from outside, and the cab between engine and ear is applied
/// here, at load, rather than baked into assets -- feedback rounds are
/// parameter tweaks. The classic voice's ogg keeps its old sound untouched,
/// and non-engine keys pass straight through.
pub fn playback_bytes(key: &str, extensions: &[&str]) -> Option<AssetBytes> {
    if !is_engine_band_key(key) {
        return asset_bytes(key, extensions);
    }
    if let Some(cached) = CAB_SEALED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(key)
    {
        return Some(cached.clone());
    }
    let (data, ext) = asset_bytes(key, extensions)?;
    let found: AssetBytes = if ext == "wav" {
        (Arc::from(cab_filter::seal_wav(&data)), ext)
    } else {
        (data, ext)
    };
    CAB_SEALED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, found.clone());
    Some(found)
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn i64_at(data: &[u8], at: usize) -> Option<i64> {
    Some(i64::from_le_bytes(data.get(at..at + 8)?.try_into().ok()?))
}

/// Playing time of a RIFF/WAVE file from its headers, as Python's `wave`
/// module measured it: data-chunk frames over the sample rate. `None` for
/// anything `wave.open` would have refused (not RIFF/WAVE, no PCM `fmt `
/// chunk, no `data` chunk).
pub(crate) fn wav_seconds(data: &[u8]) -> Option<f64> {
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12;
    let mut fmt: Option<(u16, u32, u16)> = None; // (format, rate, frame size)
    let mut data_len: Option<u32> = None;
    while pos + 8 <= data.len() {
        let id = &data[pos..pos + 4];
        let size = u32_at(data, pos + 4)? as usize;
        let body = pos + 8;
        if id == b"fmt " {
            let format = u16_at(data, body)?;
            let channels = u16_at(data, body + 2)?;
            let rate = u32_at(data, body + 4)?;
            let bits = u16_at(data, body + 14)?;
            // wave.py: sampwidth = (bits + 7) // 8; framesize = nchannels * sampwidth
            let sampwidth = (bits as u32).div_ceil(8);
            let frame = channels as u32 * sampwidth;
            // wave.py accepts WAVE_FORMAT_PCM and WAVE_FORMAT_EXTENSIBLE only.
            if format != 1 && format != 0xFFFE {
                return None;
            }
            fmt = Some((format, rate, frame as u16));
        } else if id == b"data" {
            data_len = Some(size as u32);
            break; // wave.py stops reading chunks at the data chunk
        }
        pos = body + size + (size & 1); // chunks are word-aligned
    }
    let (_format, rate, frame) = fmt?;
    let data_len = data_len?;
    if frame == 0 {
        return None;
    }
    let frames = data_len / frame as u32;
    Some(if rate > 0 {
        frames as f64 / rate as f64
    } else {
        0.0
    })
}

/// Playing time of an Ogg Vorbis stream, from its own page headers.
///
/// The last page's granule position IS the final sample number, and the
/// sample rate sits in the identification header on the first page, so the
/// whole answer is two reads and a division -- no decoding, no backend, and
/// the same number on a machine with no audio device at all.
pub(crate) fn ogg_seconds(data: &[u8]) -> f64 {
    let Some(first) = find(data, b"OggS", 0) else {
        return 0.0;
    };
    if data.len() < first + 28 {
        return 0.0;
    }
    let packet = first + 27 + data[first + 26] as usize; // page header, then its segment table
    if data.get(packet..packet + 7) != Some(b"\x01vorbis".as_slice()) {
        return 0.0;
    }
    let Some(rate) = u32_at(data, packet + 12) else {
        return 0.0;
    };
    let Some(last) = rfind(data, b"OggS") else {
        return 0.0;
    };
    if rate == 0 {
        return 0.0;
    }
    let Some(granule) = i64_at(data, last + 6) else {
        return 0.0;
    };
    if granule > 0 {
        granule as f64 / rate as f64
    } else {
        0.0
    }
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if haystack.len() < from + needle.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + from)
}

fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).rposition(|w| w == needle)
}

/// How long the clip behind `key` sounds for, in seconds.
///
/// Zero when the key resolves to nothing, or to a container this cannot
/// measure. Cached, because callers ask about the same handful of keys
/// repeatedly and the answer cannot change while the game is running.
///
/// A one-shot handed to `AudioEngine::play` comes back with no handle, so a
/// caller that needs to know when it has finished -- the Learn game sounds
/// demo, which must not lay a second copy over the first -- has this and
/// nothing else to go on.
pub fn asset_length_s(key: &str) -> f64 {
    if let Some(cached) = LENGTHS.lock().unwrap_or_else(|e| e.into_inner()).get(key) {
        return *cached;
    }
    let seconds = match asset_bytes(key, SFX_EXTENSIONS) {
        None => 0.0,
        Some((data, ext)) => {
            if ext == "ogg" {
                ogg_seconds(&data)
            } else {
                match wav_seconds(&data) {
                    Some(seconds) => seconds,
                    None => {
                        // An unreadable header is "unknown", never a crash.
                        log::warn!("Could not measure the length of {key}");
                        0.0
                    }
                }
            }
        }
    };
    LENGTHS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, seconds);
    seconds
}

/// Fail if the canonical UI sound is unreadable (packed or loose).
///
/// Used by the --smoke build check to prove frozen builds can read the
/// shipped sound pack.
pub fn verify_sound_assets() -> Result<(), AudioError> {
    if asset_bytes("ui/menu_select", SFX_EXTENSIONS).is_none() {
        return Err(AudioError::new(SOUND_ASSETS_MISSING));
    }
    Ok(())
}

/// The message [`verify_sound_assets`] fails with, named so the smoke test's
/// skip-on-an-LFS-less-checkout guard matches the same string this returns
/// rather than a copy that can drift out of step with it.
pub const SOUND_ASSETS_MISSING: &str = "Sound assets are missing or unreadable: ui/menu_select";

#[cfg(test)]
mod tests {
    use super::*;

    /// A 16-bit PCM WAV header over `frames` frames of silence.
    fn wav(rate: u32, channels: u16, frames: u32) -> Vec<u8> {
        let block = channels * 2;
        let data_len = frames * block as u32;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * block as u32).to_le_bytes());
        out.extend_from_slice(&block.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.resize(out.len() + data_len as usize, 0);
        out
    }

    #[test]
    fn wav_seconds_reads_frames_over_rate() {
        assert_eq!(wav_seconds(&wav(44100, 2, 44100)), Some(1.0));
        assert_eq!(wav_seconds(&wav(22050, 1, 11025)), Some(0.5));
        assert_eq!(wav_seconds(b"not a wav at all"), None);
        assert_eq!(wav_seconds(&wav(44100, 2, 0)), Some(0.0));
    }

    #[test]
    fn ogg_seconds_reads_the_last_granule_over_the_id_header_rate() {
        // A minimal two-page shape: an identification header page with a
        // one-segment table, then a final page carrying the granule.
        let mut ogg = Vec::new();
        ogg.extend_from_slice(b"OggS");
        ogg.extend_from_slice(&[0u8; 22]); // version .. page sequence/crc
        ogg.push(1); // one segment
        ogg.push(30); // its length
        ogg.extend_from_slice(b"\x01vorbis");
        ogg.extend_from_slice(&[0u8; 5]); // vorbis version (4) + channels (1)
        ogg.extend_from_slice(&48000u32.to_le_bytes());
        ogg.resize(ogg.len() + 14, 0); // rest of the id header
        ogg.extend_from_slice(b"OggS");
        ogg.extend_from_slice(&[0u8; 2]);
        ogg.extend_from_slice(&96000i64.to_le_bytes());
        ogg.extend_from_slice(&[0u8; 14]);
        assert!((ogg_seconds(&ogg) - 2.0).abs() < 1e-12);
        assert_eq!(ogg_seconds(b"OggS"), 0.0);
        assert_eq!(ogg_seconds(b"nothing"), 0.0);
    }

    #[test]
    fn a_music_registration_leaves_an_engine_band_cache_entry_alone() {
        let engine = "engine/test_cache_band_1200";
        let mut cache = VersionedCache::new();
        cache.insert(engine, 7u32);
        ff_core::assets_pack::register_generated_sound(
            "music/test_cache_piece",
            wav(22050, 2, 10),
            "wav",
        );
        ff_core::assets_pack::unregister_generated_sound("music/test_cache_piece");
        assert_eq!(cache.get(engine), Some(&7), "an unrelated key cleared it");
        // Registering the entry's own key does invalidate it.
        ff_core::assets_pack::register_generated_sound(engine, wav(22050, 2, 10), "wav");
        assert_eq!(cache.get(engine), None);
        ff_core::assets_pack::unregister_generated_sound(engine);
    }

    #[test]
    fn roots_are_under_the_package_directory() {
        let [licensed, committed] = asset_roots();
        assert!(committed.ends_with(Path::new("assets").join("sounds")));
        assert!(licensed.ends_with(Path::new("assets").join("sounds-licensed")));
    }

    #[test]
    fn a_checkout_finds_its_sounds_under_assets() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("the game crate sits two levels under the repo root");
        assert_eq!(package_root(), repo);
        assert_eq!(assets_dir(), repo.join("assets").join("sounds"));
        assert!(assets_dir().join("CREDITS.md").is_file());
    }

    #[test]
    fn an_installed_layout_resolves_beside_its_data_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let package = tmp.path().join("freight_fate");
        std::fs::create_dir_all(package.join("data")).unwrap();
        let root = package_root_for(&package.join("data"));
        assert_eq!(root, package);
    }
}
