//! Seamless menu music crossfading and broadcast-style internal radio segues.
//!
//! Menu music uses generous, acoustic-aware crossfades triggered at each
//! track's natural outro so menu playback flows continuously without dead air.
//! Internal radio stations use a content-typed segue matrix (songs, DJ host
//! breaks, station IDs/jingles, and commercials) so songs segue cleanly, DJ
//! breaks and jingles open without clipped first syllables, and ad stopsets
//! run tight without trailing-silence gaps.

use crate::music_cues::{track_cue, TrackCue};

/// The broadcast role of a music or radio asset key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BroadcastRole {
    /// A music track (menu bed, drive bed, or station song).
    Song,
    /// A DJ host break (`host_*`).
    Host,
    /// A sung station jingle or spoken legal ID (`id_*`).
    StationId,
    /// A commercial spot (`ad_*`).
    Ad,
}

impl BroadcastRole {
    /// Classify a track key by its asset prefix (bare or `music/`-prefixed).
    pub fn from_key(key: &str) -> Self {
        let bare = key.strip_prefix("music/").unwrap_or(key);
        if bare.starts_with("host_") {
            Self::Host
        } else if bare.starts_with("id_") {
            Self::StationId
        } else if bare.starts_with("ad_") {
            Self::Ad
        } else {
            Self::Song
        }
    }

    /// True for spoken or stinger segments that open immediately and must not
    /// have their first syllables muffled by a slow fade-in.
    pub fn is_voice_or_stinger(self) -> bool {
        !matches!(self, Self::Song)
    }
}

/// Parameters for starting a music track while fading out the current one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MusicTransition {
    /// Seek offset in seconds for the incoming track.
    pub start_s: f64,
    /// Fade-in duration for the incoming track, in milliseconds.
    pub fade_in_ms: u32,
    /// Fade-out duration for the outgoing track, in milliseconds.
    pub fade_out_ms: u32,
    /// Per-track linear loudness trim (1.0 = unity).
    pub gain: f32,
}

/// Default outgoing crossfade when callers use the legacy `play_music` API.
pub const DEFAULT_FADE_OUT_MS: u32 = 800;

impl MusicTransition {
    /// Legacy-compatible transition: start from 0, 800 ms outgoing fade, unity gain.
    pub const fn simple(fade_in_ms: u32) -> Self {
        Self {
            start_s: 0.0,
            fade_in_ms,
            fade_out_ms: DEFAULT_FADE_OUT_MS,
            gain: 1.0,
        }
    }

    /// Legacy-compatible seek transition.
    pub const fn at(fade_in_ms: u32, start_s: f64) -> Self {
        Self {
            start_s,
            fade_in_ms,
            fade_out_ms: DEFAULT_FADE_OUT_MS,
            gain: 1.0,
        }
    }
}

/// Floor on any segue trigger so short fixtures or synthetic test clips never
/// trigger at 0 s and loop infinitely.
const MIN_TRIGGER_S: f64 = 0.5;

fn cue_gain(key: &str) -> f32 {
    track_cue(key).map_or(1.0, TrackCue::linear_gain)
}

pub fn cue_lead_in_s(key: &str) -> f64 {
    track_cue(key).map_or(0.0, |c| f64::from(c.lead_in_s))
}

fn remaining_ms(cue: TrackCue, trigger_s: f64, min_ms: u32, max_ms: u32) -> u32 {
    let rem_s = (f64::from(cue.content_end_s) - trigger_s).max(0.0);
    ((rem_s * 1000.0).round() as u32).clamp(min_ms, max_ms)
}

// -- Menu music ----------------------------------------------------------------

/// Seconds into `track` at which menu rotation should begin crossfading into
/// the next track.
///
/// Uses the measured acoustic outro when available so tracks overlap smoothly
/// as the outgoing bed winds down, skipping trailing dead silence. For
/// synthesized or unmeasured tracks, overlaps the final 3.0 s.
pub fn menu_segue_trigger_s(track: &str, duration_s: f64) -> f64 {
    if let Some(cue) = track_cue(track) {
        let raw_end = f64::from(cue.content_end_s);
        let end = if duration_s > 0.0 {
            raw_end.min(duration_s)
        } else {
            raw_end
        };
        let outro = f64::from(cue.outro_start_s).min(end);
        let tail = (end - outro).max(0.0);
        let target = if tail >= 2.5 {
            (outro + 0.5).clamp(end - 5.5, end - 3.2)
        } else {
            end - 3.0
        };
        return target.clamp(MIN_TRIGGER_S.min(end * 0.5), end);
    }
    let fallback_overlap = 3.0_f64.min(duration_s * 0.25);
    (duration_s - fallback_overlap).max(MIN_TRIGGER_S.min(duration_s))
}

/// Transition parameters for advancing from `outgoing` to `incoming` in the
/// menu music rotation: a continuous, smooth crossfade with no silence gap.
pub fn plan_menu_segue(outgoing: &str, incoming: &str) -> MusicTransition {
    let incoming_cue = track_cue(incoming);
    let fade_out_ms = match track_cue(outgoing) {
        Some(cue) => {
            let trigger = menu_segue_trigger_s(outgoing, f64::from(cue.content_end_s));
            remaining_ms(cue, trigger, 2500, 5000)
        }
        None => 3000,
    };
    let fade_in_ms = match incoming_cue {
        Some(cue) if cue.intro_ramp_s >= 0.8 => 1400,
        _ => 2400,
    };
    MusicTransition {
        start_s: cue_lead_in_s(incoming),
        fade_in_ms,
        fade_out_ms,
        gain: cue_gain(incoming),
    }
}

/// Transition parameters when starting a menu track directly (initial entry,
/// night/milestone switch, or setting change).
pub fn plan_menu_start(incoming: &str, fade_in_ms: u32) -> MusicTransition {
    MusicTransition {
        start_s: cue_lead_in_s(incoming),
        fade_in_ms,
        fade_out_ms: 1600,
        gain: cue_gain(incoming),
    }
}

// -- Internal radio stations ---------------------------------------------------

/// Seconds into `outgoing` at which an internal radio station should trigger
/// the segue into `incoming`.
///
/// Accounts for both the outgoing track's measured acoustic tail/silence and
/// the broadcast pairing (song-to-song, song-to-DJ, song-to-jingle,
/// song-to-ad, ad-to-ad, break-to-song).
pub fn radio_segue_trigger_s(outgoing: &str, incoming: Option<&str>, duration_s: f64) -> f64 {
    let out_role = BroadcastRole::from_key(outgoing);
    let in_role = incoming.map_or(BroadcastRole::Song, BroadcastRole::from_key);

    if let Some(cue) = track_cue(outgoing) {
        let raw_end = f64::from(cue.content_end_s);
        let end = if duration_s > 0.0 {
            raw_end.min(duration_s)
        } else {
            raw_end
        };
        let outro = f64::from(cue.outro_start_s).min(end);
        let tail = (end - outro).max(0.0);
        let target = match (out_role, in_role) {
            (BroadcastRole::Song, BroadcastRole::Song) => {
                if tail >= 2.5 {
                    (outro + 0.6).clamp(end - 4.5, end - 2.5)
                } else {
                    (end - tail.max(2.2)).max(end - 2.8)
                }
            }
            (BroadcastRole::Song, BroadcastRole::Host) => (outro + 0.3).clamp(end - 2.4, end - 1.2),
            (BroadcastRole::Song, BroadcastRole::StationId) => (end - 1.5).max(outro),
            (BroadcastRole::Song, BroadcastRole::Ad) => end - 0.5,
            (BroadcastRole::StationId, BroadcastRole::Song) => {
                if tail >= 0.8 {
                    (outro + 0.4).clamp(end - 1.6, end - 0.5)
                } else {
                    end - 0.35
                }
            }
            (BroadcastRole::Host, BroadcastRole::Song) => end - 0.35,
            // Spoken-to-spoken (ad -> ad, ad -> ID, host -> ID, etc.):
            // crisp broadcast butt-cut right at the end of audible content.
            _ => end - 0.15,
        };
        return target.clamp(MIN_TRIGGER_S.min(end * 0.5), end);
    }

    // Fallback for synthesized Roadhouse tracks or unmeasured clips.
    let overlap = match (out_role, in_role) {
        (BroadcastRole::Song, BroadcastRole::Song) => 2.2_f64.min(duration_s * 0.2),
        (BroadcastRole::Song, BroadcastRole::Host | BroadcastRole::StationId) => {
            1.4_f64.min(duration_s * 0.15)
        }
        (BroadcastRole::Song, BroadcastRole::Ad) => 0.4_f64.min(duration_s * 0.1),
        (BroadcastRole::Host | BroadcastRole::StationId, BroadcastRole::Song) => {
            0.3_f64.min(duration_s * 0.1)
        }
        _ => 0.1_f64.min(duration_s * 0.05),
    };
    (duration_s - overlap).max(MIN_TRIGGER_S.min(duration_s))
}

/// Transition parameters for moving from `outgoing` to `incoming` on an
/// internal radio station.
pub fn plan_radio_segue(outgoing: &str, incoming: &str) -> MusicTransition {
    let out_role = BroadcastRole::from_key(outgoing);
    let in_role = BroadcastRole::from_key(incoming);
    let in_cue = track_cue(incoming);

    let (fade_in_ms, fade_out_ms) = match (out_role, in_role) {
        (BroadcastRole::Song, BroadcastRole::Song) => {
            let out_ms = match track_cue(outgoing) {
                Some(cue) => {
                    let trigger = radio_segue_trigger_s(
                        outgoing,
                        Some(incoming),
                        f64::from(cue.content_end_s),
                    );
                    remaining_ms(cue, trigger, 1600, 4000)
                }
                None => 2200,
            };
            let in_ms = match in_cue {
                Some(cue) if cue.intro_ramp_s >= 0.5 => 850,
                _ => 1400,
            };
            (in_ms, out_ms)
        }
        (BroadcastRole::Song, BroadcastRole::Host) => (40, 1800),
        (BroadcastRole::Song, BroadcastRole::StationId) => (30, 1400),
        (BroadcastRole::Song, BroadcastRole::Ad) => (40, 600),
        (BroadcastRole::Host, BroadcastRole::Song) => (300, 500),
        (BroadcastRole::StationId, BroadcastRole::Song) => {
            let out_ms = match track_cue(outgoing) {
                Some(cue) if cue.tail_s() >= 0.8 => 1100,
                _ => 400,
            };
            (250, out_ms)
        }
        (BroadcastRole::Ad, BroadcastRole::Song) => (450, 450),
        // Spoken-to-spoken (ad -> ad, ad -> id, host -> id, etc.): crisp cut.
        _ => (30, 180),
    };

    MusicTransition {
        start_s: cue_lead_in_s(incoming),
        fade_in_ms,
        fade_out_ms,
        gain: cue_gain(incoming),
    }
}

/// Transition parameters when tuning the dial onto a station (landing mid-song
/// or at the top of a break).
pub fn plan_radio_tune(incoming: &str, start_s: f64, fade_in_ms: u32) -> MusicTransition {
    let role = BroadcastRole::from_key(incoming);
    let effective_start = if start_s <= 0.0 {
        cue_lead_in_s(incoming)
    } else {
        start_s
    };
    let effective_fade_in = if role.is_voice_or_stinger() && start_s <= 0.0 {
        fade_in_ms.min(80)
    } else {
        fade_in_ms
    };
    MusicTransition {
        start_s: effective_start,
        fade_in_ms: effective_fade_in,
        fade_out_ms: 450,
        gain: cue_gain(incoming),
    }
}

/// Effective on-air duration of `outgoing` before transitioning to `incoming`,
/// matching [`radio_segue_trigger_s`] minus any trimmed leading silence so
/// `radio_rotation::cue_after` stays in lockstep with live playback.
pub fn radio_item_airtime_s(
    outgoing: &str,
    incoming: Option<&str>,
    catalog_duration_s: f64,
) -> f64 {
    let trigger = radio_segue_trigger_s(outgoing, incoming, catalog_duration_s);
    (trigger - cue_lead_in_s(outgoing)).max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broadcast_roles_classify_all_prefixes() {
        assert_eq!(BroadcastRole::from_key("menu_theme"), BroadcastRole::Song);
        assert_eq!(
            BroadcastRole::from_key("music/radio_country_two_lane"),
            BroadcastRole::Song
        );
        assert_eq!(
            BroadcastRole::from_key("host_roadhouse_01"),
            BroadcastRole::Host
        );
        assert_eq!(
            BroadcastRole::from_key("id_roadhouse_01"),
            BroadcastRole::StationId
        );
        assert_eq!(
            BroadcastRole::from_key("ad_red_hawk_travel_centers"),
            BroadcastRole::Ad
        );
    }

    #[test]
    fn menu_segue_triggers_before_trailing_silence() {
        // menu_d_major_medley has ~3.1 s trailing silence + ~3.1 s fade tail.
        let trigger = menu_segue_trigger_s("menu_d_major_medley", 238.8);
        assert!(trigger < 235.0, "should trigger before silence: {trigger}");
        assert!(trigger > 228.0, "should not cut off body: {trigger}");

        let t = plan_menu_segue("menu_theme", "menu_first_rig");
        assert!(t.fade_out_ms >= 2500);
        assert!(t.fade_in_ms >= 1400);
    }

    #[test]
    fn radio_segue_skips_long_song_and_jingle_silence() {
        // radio_gospel_set_my_burden_down has ~10 s trailing silence!
        let trigger = radio_segue_trigger_s(
            "radio_gospel_set_my_burden_down",
            Some("radio_gospel_old_church_bell"),
            226.1,
        );
        assert!(
            trigger < 216.0,
            "should trigger before 10s dead silence: {trigger}"
        );
        assert!(trigger > 210.0, "should not cut off body: {trigger}");

        // id_bigsky_02 has ~4.5 s trailing silence after a 6 s jingle.
        let id_trigger =
            radio_segue_trigger_s("id_bigsky_02", Some("radio_country_two_lane"), 12.0);
        assert!(
            id_trigger < 7.5,
            "jingle should hand off before 4.5s silence: {id_trigger}"
        );
    }

    #[test]
    fn radio_voice_and_stinger_entries_never_muffle_first_syllable() {
        let to_host = plan_radio_segue("open_road", "host_roadhouse_01");
        assert!(to_host.fade_in_ms <= 60);
        assert!(to_host.fade_out_ms >= 1500);

        let to_id = plan_radio_segue("open_road", "id_roadhouse_01");
        assert!(to_id.fade_in_ms <= 50);

        let to_ad = plan_radio_segue("open_road", "ad_red_hawk_travel_centers");
        assert!(to_ad.fade_in_ms <= 60);

        let ad_to_ad = plan_radio_segue("ad_red_hawk_travel_centers", "ad_dellas_blue_plate");
        assert!(ad_to_ad.fade_in_ms <= 50);
        assert!(ad_to_ad.fade_out_ms <= 250);

        let host_to_song = plan_radio_segue("host_roadhouse_01", "open_road");
        assert!(host_to_song.fade_in_ms <= 400);
    }

    #[test]
    fn radio_tune_protects_spoken_breaks_from_slow_fade() {
        let tune_host = plan_radio_tune("host_roadhouse_01", 0.0, 2500);
        assert!(tune_host.fade_in_ms <= 80);

        let tune_song = plan_radio_tune("open_road", 42.0, 2500);
        assert_eq!(tune_song.start_s, 42.0);
        assert_eq!(tune_song.fade_in_ms, 2500);
    }
}
