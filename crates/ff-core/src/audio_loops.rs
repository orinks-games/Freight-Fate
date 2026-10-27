//! The backend-free half of sustain-loop-with-release playback.
//!
//! A real "attack -> sustain -> release" sound -- a horn held down, a siren,
//! an engine that idles then spins down -- should loop only a short interior
//! region while it is held, then play its natural release tail once let go.
//! Plain whole-file looping instead replays the attack every cycle and never
//! lets the release ring out.
//!
//! The Python module (`freight_fate/audio_loops.py`) carried both halves: the
//! unit maths here, and a `SustainLoop` class that kept a stream seeking back
//! to the loop start each time playback reached the loop end, until
//! `release()` let it flow on through the release tail. The playing half
//! needs a device, so it lives in the game crate: `freight_fate::audio` turns
//! a [`SustainLoopSpec`] into a Hearken `LoopRegion` on the sound
//! (`start_sustain_loop`, the horn's only caller) and releases the sound from
//! it (`release_sustain_loop`), so the file plays out to its end. What
//! stays here is everything that can be checked without a device: the unit
//! conversion every caller must agree on, and the loop-point validation the
//! Python class did in its constructor.
//!
//! Port of `freight_fate/audio_loops.py` (pure parts).

use std::fmt;

/// Loop-point units: samples (needs a stream frequency) or seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopUnits {
    Samples,
    Seconds,
}

impl LoopUnits {
    /// The Python keyword the call sites pass (`units="samples"`).
    pub fn parse(units: &str) -> Option<Self> {
        match units {
            "samples" => Some(Self::Samples),
            "seconds" => Some(Self::Seconds),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Samples => "samples",
            Self::Seconds => "seconds",
        }
    }
}

/// The loop-point errors the Python code raised as `ValueError`.
#[derive(Debug, Clone, PartialEq)]
pub enum LoopPointError {
    /// `to_seconds` was asked to convert samples with no (or zero) frequency.
    MissingFrequency,
    /// `units` was neither `"samples"` nor `"seconds"`.
    UnknownUnits(String),
    /// `loop_end` is not after `loop_start`.
    InvertedPoints,
}

impl fmt::Display for LoopPointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFrequency => {
                write!(
                    f,
                    "a stream frequency is required to convert samples to seconds"
                )
            }
            Self::UnknownUnits(units) => write!(f, "unknown loop-point units: {units:?}"),
            Self::InvertedPoints => write!(f, "loop_end must be after loop_start"),
        }
    }
}

impl std::error::Error for LoopPointError {}

/// Convert a loop point to seconds.
///
/// `units` is `"samples"` (`freq` in Hz is then required) or `"seconds"`
/// (`pos` is returned unchanged).
pub fn to_seconds(pos: f64, units: &str, freq: Option<f64>) -> Result<f64, LoopPointError> {
    match LoopUnits::parse(units) {
        Some(units) => to_seconds_units(pos, units, freq),
        None => Err(LoopPointError::UnknownUnits(units.to_string())),
    }
}

/// [`to_seconds`] with the units already parsed.
pub fn to_seconds_units(
    pos: f64,
    units: LoopUnits,
    freq: Option<f64>,
) -> Result<f64, LoopPointError> {
    match units {
        LoopUnits::Seconds => Ok(pos),
        LoopUnits::Samples => match freq {
            // Python's `if not freq` treats 0.0 and None alike.
            Some(freq) if freq != 0.0 => Ok(pos / freq),
            _ => Err(LoopPointError::MissingFrequency),
        },
    }
}

/// A sustain loop described in the caller's units: the loop region the
/// backend must seek over while the sound is held.
///
/// The horn is `SustainLoopSpec::samples(11816, 12379)` (`HORN_LOOP_START`
/// / `HORN_LOOP_END` in the audio module). [`SustainLoopSpec::resolve`] is
/// what the Python `SustainLoop.__init__` did before touching BASS: convert
/// both points to seconds and refuse an inverted region.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SustainLoopSpec {
    pub loop_start: f64,
    pub loop_end: f64,
    pub units: LoopUnits,
}

/// Loop points in seconds, ready for the backend's own byte conversion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedSustainLoop {
    pub start_s: f64,
    pub end_s: f64,
}

impl SustainLoopSpec {
    pub const fn samples(loop_start: f64, loop_end: f64) -> Self {
        Self {
            loop_start,
            loop_end,
            units: LoopUnits::Samples,
        }
    }

    pub const fn seconds(loop_start: f64, loop_end: f64) -> Self {
        Self {
            loop_start,
            loop_end,
            units: LoopUnits::Seconds,
        }
    }

    /// Both points in seconds for a stream at `freq` Hz (needed for sample
    /// units only), refusing a region whose end is not after its start.
    pub fn resolve(&self, freq: Option<f64>) -> Result<ResolvedSustainLoop, LoopPointError> {
        let start_s = to_seconds_units(self.loop_start, self.units, freq)?;
        let end_s = to_seconds_units(self.loop_end, self.units, freq)?;
        if end_s <= start_s {
            return Err(LoopPointError::InvertedPoints);
        }
        Ok(ResolvedSustainLoop { start_s, end_s })
    }
}

#[cfg(test)]
mod tests {
    //! Unit-level checks for the reusable sustain-loop helper.
    use super::*;

    // The shipped horn's loop points (audio.HORN_LOOP_START / HORN_LOOP_END).
    const HORN_LOOP_START: f64 = 11816.0;
    const HORN_LOOP_END: f64 = 12379.0;

    #[test]
    fn test_to_seconds_samples_uses_frequency() {
        assert_eq!(to_seconds(44100.0, "samples", Some(44100.0)).unwrap(), 1.0);
        let got = to_seconds(11816.0, "samples", Some(44100.0)).unwrap();
        assert!((got - 11816.0 / 44100.0).abs() < 1e-12);
    }

    #[test]
    fn test_to_seconds_seconds_passthrough_ignores_frequency() {
        assert_eq!(to_seconds(0.5, "seconds", Some(1.0)).unwrap(), 0.5);
        assert_eq!(to_seconds(0.5, "seconds", None).unwrap(), 0.5);
    }

    #[test]
    fn test_to_seconds_rejects_samples_without_frequency() {
        assert_eq!(
            to_seconds(11816.0, "samples", None),
            Err(LoopPointError::MissingFrequency)
        );
        assert_eq!(
            to_seconds(11816.0, "samples", Some(0.0)),
            Err(LoopPointError::MissingFrequency)
        );
    }

    #[test]
    fn test_to_seconds_rejects_unknown_units() {
        assert_eq!(
            to_seconds(1.0, "frames", Some(44100.0)),
            Err(LoopPointError::UnknownUnits("frames".into()))
        );
    }

    // The three BASS-stream tests of the Python file
    // (`test_sustain_loop_computes_byte_positions_from_samples`,
    // `test_sustain_loop_release_is_idempotent`,
    // `test_sustain_loop_rejects_inverted_points`) need a live stream; the
    // first two belong to the game crate's audio tests. The byte arithmetic
    // and the inversion check they exercise are pinned here without a device.

    #[test]
    fn test_sustain_loop_computes_byte_positions_from_samples() {
        let spec = SustainLoopSpec::samples(HORN_LOOP_START, HORN_LOOP_END);
        let resolved = spec.resolve(Some(44100.0)).unwrap();
        // Verified against the shipped 44100 Hz horn asset: 16-bit stereo is
        // four bytes a frame, so the loop start lands on byte 47264.
        let start_frame = resolved.start_s * 44100.0;
        assert!((start_frame - HORN_LOOP_START).abs() < 1e-9);
        assert_eq!(start_frame.round() as u64 * 4, 47264);
    }

    #[test]
    fn test_sustain_loop_rejects_inverted_points() {
        let spec = SustainLoopSpec::samples(HORN_LOOP_END, HORN_LOOP_START);
        assert_eq!(
            spec.resolve(Some(44100.0)),
            Err(LoopPointError::InvertedPoints)
        );
    }
}
