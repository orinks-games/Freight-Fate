//! Where a station's rotation stands after so many seconds on the air.
//!
//! A station keeps broadcasting whether or not the truck is tuned to it, so
//! tuning back has to land where the station is *now* rather than restarting
//! its shuffled order at track one, second zero. This module replays the
//! rotation's own rules -- songs, with a host break every
//! [`RADIO_TRACKS_PER_HOST_BREAK`] songs -- forward over an airtime and hands
//! back the cue the playback loop should resume from.
//!
//! Everything here is a pure function of the station's seed key, its track
//! order and the airtime, so the same trip always hears the same station in
//! the same place. Nothing draws from a clock or an unseeded generator.

use crate::music::{crc32, RADIO_TRACKS_PER_HOST_BREAK};
use crate::music_playback::{cue_lead_in_s, radio_item_airtime_s};
use crate::radio_content::{content_duration_s, plan_break};

/// The station being walked: its identity, its shuffled track order, and the
/// seed key that fixes its host-break order for this trip.
pub struct StationRotation<'a> {
    pub station_id: &'a str,
    pub host: &'a str,
    pub playlist: &'a str,
    pub seed_key: &'a str,
    pub tracks: &'a [String],
    /// False for a station that runs song into song with no voiced breaks
    /// (the Roadhouse in Synthesized mode).
    pub breaks: bool,
}

/// A station's rotation frozen at one instant: the same fields the driving
/// state carries, so resuming is a field-for-field copy.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RotationCue {
    pub track_index: usize,
    pub tracks_since_break: usize,
    pub break_count: usize,
    pub break_queue: Vec<String>,
    pub break_pos: usize,
    /// Seconds already gone from whatever is playing right now.
    pub elapsed_s: f64,
}

impl RotationCue {
    /// The asset key on the air at this cue: a break slot when one is open,
    /// otherwise the song the rotation has reached.
    pub fn current_key(&self, tracks: &[String]) -> String {
        if !self.break_queue.is_empty() {
            return self.break_queue[self.break_pos % self.break_queue.len()].clone();
        }
        if tracks.is_empty() {
            return String::new();
        }
        tracks[self.track_index % tracks.len()].clone()
    }

    /// True while a host break, station ID or ad is the thing on the air.
    pub fn in_break(&self) -> bool {
        !self.break_queue.is_empty()
    }
}

/// Nothing in the catalog is this short; the floor only stops a bad duration
/// from turning the walk below into a loop that never ends.
const MIN_ITEM_S: f64 = 1.0;

/// Enough steps for days of airtime at the shortest item length, and a hard
/// stop if a future catalog ever hands back something degenerate.
const MAX_STEPS: usize = 20_000;

/// Longest head start a drive can open with: comfortably more than one lap of
/// any station's running order, so the opening song is never predictable.
const HEAD_START_SPAN_S: f64 = 20.0 * 60.0;

/// How long the stations have already been on the air when a drive begins.
///
/// A radio station does not sign on when the truck does, so a drive opens
/// with every station part way through its own running order rather than all
/// of them at track one, second zero. Drawn from the trip seed, so it is
/// fixed for a trip and different for the next one. One number covers every
/// station because each has its own shuffled order and its own break
/// schedule, so the same airtime puts each of them somewhere different.
pub fn initial_airtime_s(trip_seed: i64) -> f64 {
    let draw =
        crc32(format!("{trip_seed}|radio airtime").as_bytes()) as f64 / (u32::MAX as f64 + 1.0);
    draw * HEAD_START_SPAN_S
}

/// Walk `rotation` forward `airtime_s` seconds from a cold start.
///
/// The step below is the playback loop's own advance with the playing left
/// out: finish the current item, move to the next song, and open a host break
/// every [`RADIO_TRACKS_PER_HOST_BREAK`] songs. A station with no tracks
/// stays at the default cue.
pub fn cue_after(rotation: &StationRotation, airtime_s: f64) -> RotationCue {
    let mut cue = RotationCue::default();
    if rotation.tracks.is_empty() {
        return cue;
    }
    let mut remaining = airtime_s.max(0.0);
    for _ in 0..MAX_STEPS {
        let key = cue.current_key(rotation.tracks);
        let mut next_cue = cue.clone();
        step(&mut next_cue, rotation);
        let next_key = next_cue.current_key(rotation.tracks);
        let next_opt = (!next_key.is_empty()).then_some(next_key.as_str());
        let span = radio_item_airtime_s(&key, next_opt, content_duration_s(&key)).max(MIN_ITEM_S);
        if remaining < span {
            cue.elapsed_s = if remaining > 0.0 {
                cue_lead_in_s(&key) + remaining
            } else {
                0.0
            };
            return cue;
        }
        remaining -= span;
        cue = next_cue;
    }
    cue
}

/// One item of the rotation finishing, exactly as the playback loop ends one.
fn step(cue: &mut RotationCue, rotation: &StationRotation) {
    if !cue.break_queue.is_empty() {
        cue.break_pos += 1;
        if cue.break_pos < cue.break_queue.len() {
            return;
        }
        // The break is over; the song the rotation had already moved to is
        // what comes back on.
        cue.break_queue = Vec::new();
        cue.break_pos = 0;
        return;
    }
    cue.track_index += 1;
    cue.tracks_since_break += 1;
    if cue.tracks_since_break < RADIO_TRACKS_PER_HOST_BREAK || !rotation.breaks {
        return;
    }
    let queue = plan_break(
        rotation.station_id,
        rotation.host,
        rotation.playlist,
        rotation.seed_key,
        cue.break_count,
    );
    cue.tracks_since_break = 0;
    if queue.is_empty() {
        return; // a station with no voice simply runs song into song
    }
    cue.break_queue = queue;
    cue.break_pos = 0;
    cue.break_count += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::select_station_playlist;

    fn country() -> Vec<String> {
        select_station_playlist("country", "77|kbsk-billings")
    }

    fn rotation<'a>(tracks: &'a [String]) -> StationRotation<'a> {
        StationRotation {
            station_id: "kbsk-billings",
            host: "bigsky",
            playlist: "country",
            seed_key: "77|kbsk-billings",
            tracks,
            breaks: true,
        }
    }

    #[test]
    fn cold_start_is_the_top_of_the_order() {
        let tracks = country();
        let cue = cue_after(&rotation(&tracks), 0.0);
        assert_eq!(cue, RotationCue::default());
        assert_eq!(cue.current_key(&tracks), tracks[0]);
    }

    #[test]
    fn airtime_moves_the_station_on() {
        let tracks = country();
        let rot = rotation(&tracks);
        let first =
            radio_item_airtime_s(&tracks[0], Some(&tracks[1]), content_duration_s(&tracks[0]));
        // Half way through the opener.
        let cue = cue_after(&rot, first / 2.0);
        assert_eq!(cue.track_index, 0);
        assert!((cue.elapsed_s - (cue_lead_in_s(&tracks[0]) + first / 2.0)).abs() < 1e-9);
        // Just past it: the second song, barely started.
        let cue = cue_after(&rot, first + 1.0);
        assert_eq!(cue.track_index, 1);
        assert!((cue.elapsed_s - (cue_lead_in_s(&tracks[1]) + 1.0)).abs() < 1e-9);
        assert_eq!(cue.current_key(&tracks), tracks[1]);
    }

    #[test]
    fn the_host_break_lands_where_playback_would_put_it() {
        let tracks = country();
        let rot = rotation(&tracks);
        let planned = plan_break("kbsk-billings", "bigsky", "country", "77|kbsk-billings", 0);
        // Two songs in, this station's host talks.
        let two =
            radio_item_airtime_s(&tracks[0], Some(&tracks[1]), content_duration_s(&tracks[0]))
                + radio_item_airtime_s(
                    &tracks[1],
                    planned.first().map(String::as_str),
                    content_duration_s(&tracks[1]),
                );
        let cue = cue_after(&rot, two + 1.0);
        assert!(cue.in_break(), "a break should be open after two songs");
        assert_eq!(cue.break_count, 1);
        assert_eq!(cue.tracks_since_break, 0);
        assert_eq!(cue.track_index, RADIO_TRACKS_PER_HOST_BREAK);
        assert_eq!(cue.current_key(&tracks), planned[0]);
    }

    #[test]
    fn the_break_hands_back_to_the_song_it_interrupted_for() {
        let tracks = country();
        let rot = rotation(&tracks);
        let planned = plan_break("kbsk-billings", "bigsky", "country", "77|kbsk-billings", 0);
        let two =
            radio_item_airtime_s(&tracks[0], Some(&tracks[1]), content_duration_s(&tracks[0]))
                + radio_item_airtime_s(
                    &tracks[1],
                    planned.first().map(String::as_str),
                    content_duration_s(&tracks[1]),
                );
        let resume_song = &tracks[RADIO_TRACKS_PER_HOST_BREAK];
        let break_span: f64 = planned
            .iter()
            .enumerate()
            .map(|(i, k)| {
                let next = planned
                    .get(i + 1)
                    .map(String::as_str)
                    .unwrap_or(resume_song.as_str());
                radio_item_airtime_s(k, Some(next), content_duration_s(k))
            })
            .sum();
        let cue = cue_after(&rot, two + break_span + 1.0);
        assert!(!cue.in_break());
        assert_eq!(cue.track_index, RADIO_TRACKS_PER_HOST_BREAK);
        assert_eq!(cue.current_key(&tracks), *resume_song);
    }

    #[test]
    fn the_same_airtime_always_gives_the_same_cue() {
        let tracks = country();
        let rot = rotation(&tracks);
        for airtime in [0.0, 91.5, 640.0, 4_000.0, 12_345.6] {
            assert_eq!(cue_after(&rot, airtime), cue_after(&rot, airtime));
        }
        // And a long airtime still terminates rather than running the cap out.
        let long = cue_after(&rot, 6.0 * 3600.0);
        assert!(long.elapsed_s.is_finite());
        assert!(long.track_index > 0);
    }

    #[test]
    fn a_head_start_is_stable_and_differs_between_trips() {
        assert_eq!(initial_airtime_s(77), initial_airtime_s(77));
        assert_ne!(initial_airtime_s(77), initial_airtime_s(78));
        for seed in [0, 1, 77, 4242, -19] {
            assert!((0.0..HEAD_START_SPAN_S).contains(&initial_airtime_s(seed)));
        }
    }

    #[test]
    fn a_head_start_puts_each_station_somewhere_different() {
        // The point of the offset: a drive opens with stations part way into
        // their own running orders, not all of them at track one, second zero.
        let airtime = initial_airtime_s(77);
        let big_sky = select_station_playlist("country", "77|kbsk-billings");
        let rawhide = select_station_playlist("country", "77|krwl-dallas");
        let big_sky_cue = cue_after(
            &StationRotation {
                station_id: "kbsk-billings",
                host: "bigsky",
                playlist: "country",
                seed_key: "77|kbsk-billings",
                tracks: &big_sky,
                breaks: true,
            },
            airtime,
        );
        let rawhide_cue = cue_after(
            &StationRotation {
                station_id: "krwl-dallas",
                host: "rawhide",
                playlist: "country",
                seed_key: "77|krwl-dallas",
                tracks: &rawhide,
                breaks: true,
            },
            airtime,
        );
        assert!(big_sky_cue.elapsed_s > 0.0);
        assert!(big_sky_cue.track_index > 0);
        assert_ne!(
            big_sky_cue.current_key(&big_sky),
            rawhide_cue.current_key(&rawhide),
            "two country stations should not open on the same song"
        );
    }

    #[test]
    fn a_station_without_breaks_never_opens_one() {
        let tracks = country();
        let rot = StationRotation {
            breaks: false,
            ..rotation(&tracks)
        };
        for minutes in 0..240 {
            let cue = cue_after(&rot, minutes as f64 * 60.0);
            assert!(!cue.in_break(), "a break at {minutes} minutes");
            assert_eq!(cue.break_count, 0);
        }
    }

    #[test]
    fn an_empty_station_stays_put() {
        let empty: Vec<String> = Vec::new();
        let rot = StationRotation {
            station_id: "nowhere",
            host: "",
            playlist: "",
            seed_key: "77|nowhere",
            tracks: &empty,
            breaks: true,
        };
        let cue = cue_after(&rot, 900.0);
        assert_eq!(cue, RotationCue::default());
        assert_eq!(cue.current_key(&empty), "");
    }
}
