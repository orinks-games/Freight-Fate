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
use super::STEREO_ENGINE_BANDS;

pub(super) const CAB_SAMPLE_RATE: u32 = 48_000;
pub(super) const CAB_START_MAX_FRAMES: usize = CAB_SAMPLE_RATE as usize * 10;
pub(super) const CAB_LOOP_FADE_FRAMES: usize = 384;
const CAB_FRAME_BYTES: usize = 4;

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
        if let Some((band, frames)) = key.split_once('@') {
            if STEREO_ENGINE_BANDS.iter().any(|(key, _)| *key == band) {
                let frames = frames.parse::<usize>().ok()?;
                let (bytes, _) = Self::sfx(band)?;
                return cab_band_with_preroll(&bytes, frames);
            }
        }
        FfClips::sfx(key).map(|(bytes, _ext)| bytes)
    }
}

/// The cab overlay uses stereo PCM16 at 48 kHz. Read chunks so a WAV
/// with producer metadata before its samples still works.
fn cab_wav_samples(bytes: &[u8]) -> Option<&[u8]> {
    if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }
    let mut position = 12;
    let mut pcm = false;
    while let Some(header) = bytes.get(position..position + 8) {
        let size = u32::from_le_bytes(header[4..8].try_into().ok()?) as usize;
        let start = position + 8;
        let chunk = bytes.get(start..start.checked_add(size)?)?;
        match &header[..4] {
            b"fmt " => {
                pcm = chunk.get(..4)? == [1, 0, 2, 0]
                    && chunk.get(4..8)? == CAB_SAMPLE_RATE.to_le_bytes()
                    && chunk.get(12..16)? == [4, 0, 16, 0];
            }
            b"data" => {
                return (pcm && !chunk.is_empty() && chunk.len() % CAB_FRAME_BYTES == 0)
                    .then_some(chunk);
            }
            _ => {}
        }
        position = start + size + (size & 1);
    }
    None
}

pub(super) fn cab_start_frames(bytes: &[u8]) -> Option<usize> {
    let frames = cab_wav_samples(bytes)?.len() / CAB_FRAME_BYTES;
    (frames <= CAB_START_MAX_FRAMES).then_some(frames)
}

fn cab_band_with_preroll(bytes: &[u8], frames: usize) -> Option<Arc<[u8]>> {
    if !(1..=CAB_START_MAX_FRAMES).contains(&frames) {
        return None;
    }
    let samples = cab_wav_samples(bytes)?;
    let fade_bytes = CAB_LOOP_FADE_FRAMES * CAB_FRAME_BYTES;
    let head = samples.get(..fade_bytes)?;
    let silence_bytes = frames * CAB_FRAME_BYTES;
    let mut out = ff_core::wav::pcm16_header(
        (silence_bytes + samples.len() + fade_bytes) / 2,
        2,
        CAB_SAMPLE_RATE,
    );
    out.resize(out.len() + silence_bytes, 0);
    for (i, frame) in head.as_chunks::<CAB_FRAME_BYTES>().0.iter().enumerate() {
        let gain = i as f64 / (CAB_LOOP_FADE_FRAMES - 1) as f64;
        for &sample in frame.as_chunks::<2>().0 {
            let value = i16::from_le_bytes(sample);
            out.extend_from_slice(&((f64::from(value) * gain).round() as i16).to_le_bytes());
        }
    }
    // The repeated region starts after the faded entry and wraps from
    // the original head to the replay of its next sample.
    out.extend_from_slice(&samples[fade_bytes..]);
    out.extend_from_slice(head);
    Some(out.into())
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
    fn cab_band_is_silent_for_the_intro_and_repeats_the_original_samples() {
        let intro_frames = 960;
        let pcm: Vec<i16> = (0..1200).flat_map(|i| [100 + i, -100 - i]).collect();
        let wav = ff_core::wav::pcm16_wav(&pcm, 2, CAB_SAMPLE_RATE);
        assert_eq!(cab_start_frames(&wav), Some(1200));
        let prepared = cab_band_with_preroll(&wav, intro_frames).unwrap();
        let source = cab_wav_samples(&wav).unwrap();
        let samples = cab_wav_samples(&prepared).unwrap();
        let silence_end = intro_frames * CAB_FRAME_BYTES;
        let fade_end = silence_end + CAB_LOOP_FADE_FRAMES * CAB_FRAME_BYTES;
        let wrap = CAB_LOOP_FADE_FRAMES * CAB_FRAME_BYTES;
        assert!(samples[..silence_end].iter().all(|&byte| byte == 0));
        assert_eq!(
            &samples[silence_end..silence_end + CAB_FRAME_BYTES],
            &[0; 4]
        );
        assert_eq!(
            &samples[fade_end..silence_end + source.len()],
            &source[wrap..]
        );
        assert_eq!(&samples[silence_end + source.len()..], &source[..wrap]);
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
