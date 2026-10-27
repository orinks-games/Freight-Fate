//! Live radio on the Hearken backend: a station is a [`Stream`] that
//! connects on its own thread, played as a feed on the music channel.
//!
//! The rules are the ones the radio layer was written against:
//!
//! * **Opening never blocks a frame.** The stream connects behind
//!   [`Stream::open_with`]; `update` collects it once it plays.
//! * **One owner for retries.** Hearken's own reconnect schedule is off
//!   (`retry_delays` empty): the driving state re-tunes a silent radio every
//!   9 s and a playlist holds and retries, so a second schedule underneath
//!   would double every retry.
//! * **A failed connect is reported on the next call for the same URL**,
//!   which is when the driving state's reconnect loop retries; a later tune
//!   back to the station starts a fresh attempt.
//! * **A station fades in only once it is audible**, over the fade the
//!   caller asked for, so the ramp protects the first sample instead of
//!   running out while the stream buffers.
//! * **Titles go through the game's own parsers**, over the metadata exactly
//!   as the station sent it, so the encoder's ad fields are stripped and a
//!   Latin-1 title reads as it always has.

use std::time::{Duration, Instant};

use hearken::audio::{Bus, Sound};
use hearken::radio::{RawTitle, Stream, StreamOptions, StreamState};

use super::hearken_backend::HearkenBackend;
use super::{
    parse_hls_stream_title, parse_icy_stream_title, parse_ogg_stream_title, AudioError,
    RADIO_CONNECT_TIMEOUT_MS, RADIO_NET_BUFFER_MS, RADIO_NET_PREBUF_PERCENT, RADIO_READ_TIMEOUT_MS,
};

/// What a radio station's server sees as the listener's player.
pub(crate) fn radio_user_agent() -> String {
    format!(
        "FreightFate/{} (+https://orinks.net/freight-fate)",
        crate::app::version()
    )
}

/// How a station is called: thirty seconds to answer, thirty more of
/// silence before a station that answered counts as gone, a 1.5 s buffer that starts playing at 300 ms, no reconnects of its own,
/// and the game's own name on the request.
pub fn radio_stream_options() -> StreamOptions {
    let buffer_ms = u64::from(RADIO_NET_BUFFER_MS);
    StreamOptions {
        timeout: Duration::from_millis(u64::from(RADIO_CONNECT_TIMEOUT_MS)),
        stall: Duration::from_millis(u64::from(RADIO_READ_TIMEOUT_MS)),
        retry_delays: Vec::new(),
        buffer: Duration::from_millis(buffer_ms),
        prebuffer: Duration::from_millis(buffer_ms * u64::from(RADIO_NET_PREBUF_PERCENT) / 100),
        user_agent: radio_user_agent(),
        ..StreamOptions::default()
    }
}

/// A station still connecting: not yet on the music channel.
pub(super) struct RadioConnect {
    url: String,
    stream: Stream,
    fade_ms: u32,
    requested_at: Instant,
}

/// The station on the music channel, beside its feed's sound (its URL is
/// the music track).
pub(super) struct RadioPlaying {
    pub(super) stream: Stream,
}

impl HearkenBackend {
    /// Tune a live internet stream. Returns at once: the station connects
    /// off the game thread and `update` puts it on the music channel when it
    /// plays.
    pub(super) fn play_radio_stream(&mut self, url: &str, fade_ms: u32) -> Result<(), AudioError> {
        // Same URL only dedupes while the stream is actually producing audio;
        // a stalled or dead connection must be torn down and recreated, or a
        // re-tune to the same station silently does nothing.
        if self.music_track.as_deref() == Some(url) && self.music_playing() {
            return Ok(());
        }
        if self.radio_connecting_url() == Some(url) {
            return Ok(()); // already on its way; silence is the caller's retry cue
        }
        if self.radio_failed_url.as_deref() == Some(url) {
            // The last attempt never produced audio; say so now, and let a
            // later tune back to this station start a fresh attempt.
            self.radio_failed_url = None;
            return Err(AudioError::new("radio stream unavailable"));
        }
        self.cancel_radio_connect();
        self.fade_out_music();
        self.radio_connect = Some(RadioConnect {
            url: url.to_string(),
            stream: Stream::open_with(url, radio_stream_options()),
            fade_ms,
            requested_at: Instant::now(),
        });
        Ok(())
    }

    /// Put a station that has started playing on the music channel, or note
    /// one that failed; game thread only, from `update`.
    pub fn collect_radio_stream(&mut self) {
        let Some(connect) = self.radio_connect.as_ref() else {
            return;
        };
        match connect.stream.state() {
            StreamState::Connecting => return,
            StreamState::Failed(reason) => {
                log::info!("Radio stream unavailable: {} ({reason})", connect.url);
                self.radio_failed_url = Some(connect.url.clone());
                self.radio_connect = None;
                return;
            }
            StreamState::Playing | StreamState::Reconnecting => {}
        }
        let Some(connect) = self.radio_connect.take() else {
            return;
        };
        if self.music_track.is_some() {
            // Something else claimed the music channel while the station was
            // connecting (a menu bed, another tune); the late arrival loses.
            return;
        }
        let RadioConnect {
            url,
            stream,
            fade_ms,
            requested_at,
        } = connect;
        // From silence, now that it is audible.
        let sound = self
            .audio
            .start_feed_at_volume(stream.feed(), None, Bus::MUSIC, 0.0);
        if sound == Sound::NONE {
            log::warn!("Could not play radio stream: {url}");
            self.radio_failed_url = Some(url);
            return;
        }
        let level = self.buses.music_level() as f32;
        self.audio.set_volume_over(sound, level, fade_ms);
        log::info!(
            "Radio tune timing: audible_start_ms={}, fade_ms={fade_ms}",
            requested_at.elapsed().as_millis(),
        );
        self.music_sound = Some(sound);
        self.music_length = None;
        self.music_track = Some(url);
        self.radio = Some(RadioPlaying { stream });
    }

    /// Orphan any connect in flight; its stream is dropped, not played.
    pub(super) fn cancel_radio_connect(&mut self) {
        self.radio_connect = None;
        self.radio_failed_url = None;
    }

    /// Whether the station on the music channel is producing audio: a
    /// stream that dropped is not, though its buffer may still be playing
    /// out, so a re-tune rebuilds it.
    pub(super) fn radio_playing(&self) -> bool {
        self.radio
            .as_ref()
            .is_none_or(|radio| radio.stream.state() == StreamState::Playing)
    }

    /// The song title the playing stream reports, wherever its kind of
    /// stream carries one: the ICY metadata block (MP3 and AAC stations),
    /// the codec's comments (Ogg, Opus, FLAC), or the playlist entry (HLS).
    pub(super) fn radio_now_playing(&self) -> Option<String> {
        if !self.music_playing() {
            return None;
        }
        match self.radio.as_ref()?.stream.raw_title()? {
            RawTitle::Icy(block) => parse_icy_stream_title(Some(&block)),
            RawTitle::Comments(comments) => parse_ogg_stream_title(&comments),
            RawTitle::Extinf(extinf) => parse_hls_stream_title(&extinf),
        }
    }

    // -- inspection ------------------------------------------------------------

    /// The URL a station is still connecting to, if any.
    pub fn radio_connecting_url(&self) -> Option<&str> {
        self.radio_connect.as_ref().map(|c| c.url.as_str())
    }

    /// The URL whose last connect failed and has not been reported yet.
    pub fn radio_failed_url(&self) -> Option<&str> {
        self.radio_failed_url.as_deref()
    }

    /// The station on the music channel, for tests and diagnostics.
    pub fn radio_stream(&self) -> Option<&Stream> {
        self.radio.as_ref().map(|radio| &radio.stream)
    }

    /// Collect for up to `timeout` until no station is still connecting
    /// (a test's stand-in for frames passing).
    pub fn settle_radio(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            self.collect_radio_stream();
            if self.radio_connect.is_none() || Instant::now() >= deadline {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stations_see_freight_fate_and_its_version() {
        let agent = radio_user_agent();
        assert!(
            agent.starts_with(&format!("FreightFate/{} ", crate::app::version())),
            "{agent}"
        );
        assert!(!agent.contains("BASS"), "{agent}");
        assert!(!agent.contains("Hearken"), "{agent}");
    }

    #[test]
    fn stations_are_called_with_the_tune_policy() {
        let options = radio_stream_options();
        assert_eq!(options.timeout, Duration::from_secs(30));
        assert_eq!(options.stall, Duration::from_secs(30));
        assert_eq!(options.prebuffer, Duration::from_millis(300));
        assert_eq!(options.buffer, Duration::from_millis(1_500));
        assert!(options.retry_delays.is_empty());
        assert_eq!(options.user_agent, radio_user_agent());
    }
}
