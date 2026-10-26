//! Where the Hearken backend's clips come from: Freight Fate's own lookup,
//! handed to Hearken as bytes.
//!
//! Hearken knows nothing of the game's packs. The generated-sound registry,
//! `sounds.pak`, the licensed overlay and the loose tree are searched here in
//! the order [`playback_bytes`] and [`asset_bytes`] always searched them,
//! engine cuts come back cab-sealed exactly as BASS was given them, and a
//! miss is never cached: a sound the game generates and registers as it runs
//! (the lane guide tone, the siren signature, a synthesized music piece) is
//! heard the first time it is asked for after it exists.

use std::sync::Arc;

use hearken::audio::ClipSource;

use super::assets::{asset_bytes, playback_bytes, AssetBytes, MUSIC_EXTENSIONS, SFX_EXTENSIONS};

/// The game's sound lookup as a Hearken [`ClipSource`]. Stateless, so it
/// caches nothing, misses included.
#[derive(Debug, Default, Clone, Copy)]
pub struct FfClips;

impl FfClips {
    /// An effect or loop: the bytes BASS played for `key`, and their
    /// extension.
    pub fn sfx(key: &str) -> Option<AssetBytes> {
        playback_bytes(key, SFX_EXTENSIONS)
    }

    /// A shipped music track by its catalog name: Channel 3000's clips from
    /// their own pack under `c3k/`, every other track under `music/`.
    pub fn music(track: &str) -> Option<AssetBytes> {
        asset_bytes(
            &ff_core::channel3000::music_asset_key(track),
            MUSIC_EXTENSIONS,
        )
    }
}

impl ClipSource for FfClips {
    fn load(&mut self, key: &str) -> Option<Arc<[u8]>> {
        FfClips::sfx(key).map(|(bytes, _ext)| bytes)
    }
}

/// The sample rate a clip was recorded at, read from its own headers: what
/// BASS reported as a stream's starting `FREQ`, which sample-unit loop
/// points are counted in. WAV and Ogg Vorbis read their headers; Opus always
/// decodes at 48 kHz. `None` for anything else.
pub fn clip_sample_rate(bytes: &[u8]) -> Option<f64> {
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        let mut at = 12;
        while at + 8 <= bytes.len() {
            let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().ok()?) as usize;
            if &bytes[at..at + 4] == b"fmt " {
                let rate = bytes.get(at + 12..at + 16)?;
                return Some(f64::from(u32::from_le_bytes(rate.try_into().ok()?)));
            }
            at += 8 + size + (size & 1);
        }
        return None;
    }
    if bytes.len() >= 28 && &bytes[0..4] == b"OggS" {
        let packet = 27 + bytes[26] as usize;
        let head = bytes.get(packet..packet + 16)?;
        if &head[..7] == b"\x01vorbis" {
            let rate = u32::from_le_bytes(head[12..16].try_into().ok()?);
            return (rate > 0).then_some(f64::from(rate));
        }
        if &head[..8] == b"OpusHead" {
            return Some(48_000.0);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wav_reports_its_fmt_rate() {
        let wav = hearken::synth::wav_bytes(&[0.0; 100], 22_050);
        assert_eq!(clip_sample_rate(&wav), Some(22_050.0));
        assert_eq!(clip_sample_rate(b"not audio"), None);
    }

    #[test]
    fn a_generated_sound_registered_after_a_miss_resolves() {
        let key = "test/hearken_clips_late_registration";
        let mut clips = FfClips;
        assert!(clips.load(key).is_none());
        let wav = hearken::synth::wav_bytes(&[0.0; 10], 44_100);
        ff_core::assets_pack::register_generated_sound(key, wav.clone(), "wav");
        assert_eq!(clips.load(key).as_deref(), Some(&wav[..]));
    }
}
