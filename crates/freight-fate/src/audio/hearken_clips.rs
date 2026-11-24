//! Where the Hearken backend's clips come from: Freight Fate's own lookup,
//! handed to Hearken as bytes.
//!
//! Hearken knows nothing of the game's packs. The generated-sound registry,
//! `sounds.pak`, the licensed overlay and the loose tree are searched here in
//! the order [`playback_bytes`] and [`asset_bytes`] always searched them,
//! stereo engine loops use tuned phase-matched boundaries, and a miss is never
//! cached:
//! a sound the game generates and registers as it runs (the lane guide tone,
//! the siren signature, a synthesized music piece) is heard the first time it
//! is asked for after it exists.

use std::sync::Arc;

use hearken::audio::ClipSource;

use super::assets::{asset_bytes, playback_bytes, AssetBytes, MUSIC_EXTENSIONS, SFX_EXTENSIONS};
use super::STEREO_ENGINE_BANDS;

pub(super) const CAB_SAMPLE_RATE: u32 = 48_000;
pub(super) const CAB_START_MAX_FRAMES: usize = CAB_SAMPLE_RATE as usize * 10;
const CAB_LOOP_CROSSFADE_FRAMES: usize = 4_800;
const CAB_FRAME_BYTES: usize = 4;
// These boundaries are zero-crossing and phase matches in the five approved
// cab recordings. Cropping there avoids the comb filtering that a long
// head/tail crossfade added to the steady engine note. The frame count guards
// against applying an old recording's edit points to a replacement.
const CAB_LOOP_RANGES: [(&str, usize, usize, usize); 5] = [
    ("engine_stereo/idle", 141_773, 28_359, 108_091),
    ("engine_stereo/low", 115_761, 804, 84_393),
    ("engine_stereo/mid", 121_569, 3_925, 118_938),
    ("engine_stereo/midhigh", 175_932, 1_923, 141_387),
    ("engine_stereo/high", 136_293, 1_466, 128_564),
];

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
                return cab_band_with_preroll(band, &bytes, frames);
            }
        }
        if STEREO_ENGINE_BANDS.iter().any(|(band, _)| *band == key) {
            let (bytes, _) = Self::sfx(key)?;
            return cab_loop_wav(key, &bytes);
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

fn cab_band_with_preroll(key: &str, bytes: &[u8], frames: usize) -> Option<Arc<[u8]>> {
    if !(1..=CAB_START_MAX_FRAMES).contains(&frames) {
        return None;
    }
    let samples = cab_loop_samples(key, bytes)?;
    let silence_bytes = frames * CAB_FRAME_BYTES;
    let mut out =
        ff_core::wav::pcm16_header((silence_bytes + samples.len()) / 2, 2, CAB_SAMPLE_RATE);
    out.resize(out.len() + silence_bytes, 0);
    out.extend_from_slice(&samples);
    Some(out.into())
}

pub(super) fn cab_loop_wav(key: &str, bytes: &[u8]) -> Option<Arc<[u8]>> {
    let samples = cab_loop_samples(key, bytes)?;
    let mut out = ff_core::wav::pcm16_header(samples.len() / 2, 2, CAB_SAMPLE_RATE);
    out.extend_from_slice(&samples);
    Some(out.into())
}

fn cab_loop_samples(key: &str, bytes: &[u8]) -> Option<Vec<u8>> {
    let samples = cab_wav_samples(bytes)?;
    let frames = samples.len() / CAB_FRAME_BYTES;
    if let Some(&(_, expected, start, end)) = CAB_LOOP_RANGES
        .iter()
        .find(|(candidate, _, _, _)| *candidate == key)
    {
        if frames == expected {
            return Some(
                samples
                    .get(start * CAB_FRAME_BYTES..end * CAB_FRAME_BYTES)?
                    .to_vec(),
            );
        }
    }
    let fade_bytes = CAB_LOOP_CROSSFADE_FRAMES * CAB_FRAME_BYTES;
    let middle = samples.get(fade_bytes..samples.len().checked_sub(fade_bytes)?)?;
    let tail = samples.get(samples.len() - fade_bytes..)?;
    let head = samples.get(..fade_bytes)?;
    let mut out = Vec::with_capacity(samples.len() - fade_bytes);
    out.extend_from_slice(middle);
    for i in 0..CAB_LOOP_CROSSFADE_FRAMES {
        let progress = (i + 1) as f64 / (CAB_LOOP_CROSSFADE_FRAMES + 1) as f64;
        let tail_gain = (progress * std::f64::consts::FRAC_PI_2).cos();
        let head_gain = (progress * std::f64::consts::FRAC_PI_2).sin();
        for channel in 0..2 {
            let offset = (i * 2 + channel) * 2;
            let tail_sample = i16::from_le_bytes(tail[offset..offset + 2].try_into().ok()?);
            let head_sample = i16::from_le_bytes(head[offset..offset + 2].try_into().ok()?);
            let sample = (f64::from(tail_sample) * tail_gain + f64::from(head_sample) * head_gain)
                .round()
                .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16;
            out.extend_from_slice(&sample.to_le_bytes());
        }
    }
    Some(out)
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
    fn cab_band_is_silent_for_the_intro_and_uses_the_prepared_loop() {
        let intro_frames = 9_600;
        let pcm: Vec<i16> = (0..12_000).flat_map(|i| [100 + i, -100 - i]).collect();
        let wav = ff_core::wav::pcm16_wav(&pcm, 2, CAB_SAMPLE_RATE);
        assert_eq!(cab_start_frames(&wav), Some(12_000));
        let prepared = cab_band_with_preroll("engine_stereo/test", &wav, intro_frames).unwrap();
        let samples = cab_wav_samples(&prepared).unwrap();
        let silence_end = intro_frames * CAB_FRAME_BYTES;
        assert!(samples[..silence_end].iter().all(|&byte| byte == 0));
        assert_eq!(
            &samples[silence_end..],
            cab_loop_samples("engine_stereo/test", &wav)
                .unwrap()
                .as_slice()
        );
    }

    #[test]
    fn cab_loop_crossfades_its_tail_into_its_head() {
        let pcm: Vec<i16> = (0..12_000).flat_map(|i| [100 + i, -100 - i]).collect();
        let wav = ff_core::wav::pcm16_wav(&pcm, 2, CAB_SAMPLE_RATE);
        let looped = cab_loop_samples("engine_stereo/test", &wav).unwrap();
        assert_eq!(
            looped.len(),
            (12_000 - CAB_LOOP_CROSSFADE_FRAMES) * CAB_FRAME_BYTES
        );
        assert_eq!(
            &looped[..CAB_FRAME_BYTES],
            &cab_wav_samples(&wav).unwrap()[CAB_LOOP_CROSSFADE_FRAMES * CAB_FRAME_BYTES..]
                [..CAB_FRAME_BYTES]
        );
    }

    #[test]
    fn approved_cab_loop_uses_its_phase_matched_boundaries() {
        let (_, frames, start, end) = CAB_LOOP_RANGES[0];
        let pcm: Vec<i16> = (0..frames)
            .flat_map(|frame| {
                let sample = (frame % i16::MAX as usize) as i16;
                [sample, -sample]
            })
            .collect();
        let wav = ff_core::wav::pcm16_wav(&pcm, 2, CAB_SAMPLE_RATE);
        let looped = cab_loop_samples("engine_stereo/idle", &wav).unwrap();
        assert_eq!(looped.len(), (end - start) * CAB_FRAME_BYTES);
        assert_eq!(
            &looped[..CAB_FRAME_BYTES],
            &cab_wav_samples(&wav).unwrap()[start * CAB_FRAME_BYTES..(start + 1) * CAB_FRAME_BYTES]
        );
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
