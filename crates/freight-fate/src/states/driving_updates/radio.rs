//! The in-cab radio: reception, the FM fringe, station rotation, personal
//! playlists, the dial keys, and the badges the dial earns.

use ff_core::music::RADIO_TRACKS_PER_HOST_BREAK;
use ff_core::pyrandom::PyRandom;
use ff_core::radio::{
    effective_range_miles, is_stream_entry, signal_volume_factor, station_identity,
    truck_elevation_ft, truck_position, RadioAction, RadioPlaybackError, RadioReception,
    RadioStation, PERSONAL_PLAYLIST_SOURCE_TYPE,
};
use ff_core::radio_content::{content_duration_s, plan_break};
use ff_core::radio_rotation::{cue_after, RotationCue, StationRotation};
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::audio::{VolumeUpdate, CH_RADIO_FX, RADIO_TUNE_FADE_MS};
use crate::states::driving::{DrivingState, PlaylistShuffleLap};
use crate::states::driving_core::*;
use crate::states::driving_updates::{
    FM_DEFAULT_MHZ, FRINGE_BED_MAX_VOLUME, FRINGE_BED_SIGNAL, PICKET_DUCK, PICKET_MAX_RATE_HZ,
    PICKET_MIN_RATE_HZ, PICKET_SIGNAL, PLAYLIST_CONNECT_HOLD_S, PLAYLIST_CONNECT_TRIES,
    PLAYLIST_FADE_HOLD_S, PLAYLIST_RETRY_S, RADIO_VOLUME_STEP,
};

/// One lap's order for a shuffled playlist: `random.shuffle` on the entry
/// indices, then, when `avoid` is the track that just ended, the first slot
/// swapped away from it so a new lap never repeats it back to back.
fn shuffled_lap(
    trip_seed: i64,
    station_id: &str,
    lap: u64,
    len: usize,
    avoid: Option<usize>,
) -> Vec<usize> {
    let mut rng =
        PyRandom::new_from_str(&format!("{trip_seed}:playlist-shuffle:{station_id}:{lap}"));
    let mut order: Vec<usize> = (0..len).collect();
    // CPython's random.shuffle: for i in reversed(range(1, len)): j = randbelow(i + 1); swap.
    for i in (1..len).rev() {
        let j = rng.randbelow(i as u64 + 1) as usize;
        order.swap(i, j);
    }
    if len > 1 && avoid.is_some() && order.first().copied() == avoid {
        order.swap(0, 1);
    }
    order
}

impl DrivingState {
    /// Keep the radio spinning while a menu covers the drive.
    ///
    /// A paused rig is still a cab with the radio on: the station keeps
    /// rotating songs and host breaks under the pause menu instead of going
    /// silent when the current bed runs out. Day/night flavor stays as it
    /// was when the menu opened; it catches up when driving resumes.
    pub fn tick_drive_music(&mut self, ctx: &mut GameContext, dt: f64) {
        self.apply_radio_voicing(ctx);
        self.advance_radio_airtime(dt);
        self.sync_radio_power(ctx); // a rest-menu shutdown kills the radio too
        if self.radio.enabled && self.trip.truck.engine_on {
            let night = self.music_night;
            self.update_radio_playback(ctx, night, dt);
        }
    }

    /// The truck's place on the dial, for reception and the picket rate.
    fn radio_position(&self, ctx: &GameContext) -> (Option<(f64, f64)>, Option<f64>) {
        let view = RadioRouteView(&self.route);
        let world = ctx.world;
        let position = truck_position(Some(&view), self.trip.position_mi, &|city| {
            world.city(city).ok().map(|city| (city.lat, city.lon))
        });
        let elevation = truck_elevation_ft(Some(&view), self.trip.position_mi);
        (position, elevation)
    }

    /// Fade ranged stations with distance and retune when they drop out.
    pub fn update_radio_reception(&mut self, ctx: &mut GameContext, dt: f64) {
        self.radio_signal_timer -= dt.max(0.0);
        if self.radio_signal_timer > 0.0 {
            return;
        }
        self.radio_signal_timer = 1.5;
        // What the cab is playing, not what the dial resolves to. The frame's
        // settings sync has already moved the truck's radio position and
        // re-pointed the dial by the time this tick runs, so a resolving read
        // here answered with the fallback on both sides of the comparison:
        // the station left the map with nothing said, no static, no retune,
        // and its dead stream carried on at the fallback's full volume while
        // the drivers board named a station the cab never played (owner,
        // I-35 south of Minneapolis, 2026-09-16: KVSC "came back", and the
        // board had him on the Eagle). Ported from Python, which had the
        // same frame order and the same hole.
        let on_air = self.radio.station_by_id(&self.radio_station_id).cloned();
        let (position, elevation) = self.radio_position(ctx);
        self.radio.update_position(position, elevation);
        let reception = self.radio.current_reception();
        // Sites of one station are one station: a stronger sibling taking the
        // dial is the signal getting better, not a station lost.
        let lost = on_air.filter(|before| {
            before.id != reception.station.id
                && station_identity(before) != station_identity(&reception.station)
        });
        if let Some(before) = lost {
            // the tuned station fell past its range contour mid-drive
            ctx.award_driving_achievement("radio_faded_out");
            self.radio_states_held.clear();
            ctx.audio.play_with("radio/static_burst", 0.5, 0.0);
            // A driver on local radio stays on local radio: the strongest
            // station the truck can hear from here takes the dial, and the
            // in-house playlist is only where it lands when nothing is on
            // the air (Brandon, 2026-09-17).
            let next = self.radio.strongest_terrestrial(&before);
            let landing = next
                .as_ref()
                .map_or(SAFE_ROUTE_PLAYLIST.to_string(), |r| r.station.id.clone());
            let action = self.with_radio_backend(ctx, |radio, backend| {
                radio.select_station(&landing, Some(backend))
            });
            // The dead station's fringe must die with it: without this the
            // cached signal keeps the hiss bed and pickets crackling over
            // the fallback -- and its picket duck holding the volume down --
            // until the next reception tick.
            self.radio_fringe_signal = None;
            self.stop_radio_fringe(ctx);
            self.write_radio_settings(ctx);
            // Named from what the radio actually landed on: a stream that
            // would not open has already been swapped for the fallback.
            let retuned = next.is_some() && !action.fallback_used;
            let line = if retuned {
                format!(
                    "{} faded out of range. Tuned to {}, the strongest signal here.",
                    before.display_name(),
                    action.station.display_name()
                )
            } else {
                format!(
                    "{} faded out of range. Falling back to {}.",
                    before.display_name(),
                    action.station.display_name()
                )
            };
            ctx.say_event_with(line, SayEvent::queued().category(SpeechCategory::Status));
            return;
        }
        self.radio_signal_factor = signal_volume_factor(&reception);
        self.track_radio_badges(ctx, &reception);
        self.apply_radio_volume(ctx);
        if reception.station.real_stream && !ctx.audio.music_playing() {
            // A dead stream is a silent radio, not a fringe one -- no program,
            // so no crackle. Dock and menu beds borrow the music channel and
            // nothing restarts the stream afterward (a network stall ends the
            // same way), so quietly re-tune it here; if the station is truly
            // unreachable the radio's own fallback machinery speaks the switch.
            self.radio_now_playing = None;
            self.radio_reconnect_timer -= 1.5;
            if self.radio_reconnect_timer <= 0.0 {
                self.radio_reconnect_timer = 9.0;
                let action =
                    self.with_radio_backend(ctx, |radio, backend| radio.play(Some(backend), ""));
                if action.fallback_used {
                    self.write_radio_settings(ctx);
                }
                if action.fallback_used || action.retried {
                    ctx.say_event_with(
                        action.message,
                        SayEvent::queued().category(SpeechCategory::Status),
                    );
                }
            }
            self.radio_fringe_signal = None;
            return;
        }
        self.radio_reconnect_timer = 0.0;
        // What the stream says it is playing, read on the same tick that
        // judges its signal. Only a live connection carries song metadata.
        self.radio_now_playing = if self.station_sends_song_info(&reception.station) {
            ctx.audio.radio_now_playing()
        } else {
            None
        };
        // Cache what the per-frame fringe renderer needs: thinning signal and
        // the dial frequency (for the picket flutter rate). Satellite and
        // built-in stations have no fringe.
        let signal = reception.signal;
        if signal > 0.0 && !reception.station.always_available {
            self.radio_fringe_signal = Some(signal);
            self.radio_fringe_freq = reception.station.frequency_mhz;
        } else {
            self.radio_fringe_signal = None;
        }
    }

    // -- FM fringe: hiss bed + picket-fence flutter ---------------------------
    //
    // The hiss bed creeps in below full quieting and deepens with distance;
    // pickets are sharp splashes of noise punching through the program (FM
    // capture is a threshold, so the gating is abrupt -- owner ruling
    // 2026-07-23). Their arrival is exponential around the physical Rayleigh
    // rate 2v/lambda, never metronomic: a fixed 18 Hz tremolo sounds like a
    // helicopter, not a fringe FM signal.

    pub fn update_radio_fringe(&mut self, ctx: &mut GameContext, dt: f64) {
        let Some(signal) = self
            .radio_fringe_signal
            .filter(|_| ctx.audio.music_playing())
        else {
            // No station, satellite/built-in, or a dead stream: a silent
            // radio has no fringe (the Merced ghost-hiss lesson).
            self.stop_radio_fringe(ctx);
            return;
        };
        let depth = ((FRINGE_BED_SIGNAL - signal) / FRINGE_BED_SIGNAL).clamp(0.0, 1.0);
        if depth <= 0.0 {
            self.stop_radio_fringe(ctx);
            return;
        }
        // start_loop dedupes on a running key, so this doubles as the volume
        // update AND self-heals after anything stopped the channel. The radio
        // knob scales the hiss along with the program it degrades.
        ctx.audio.start_loop_with(
            CH_RADIO_FX,
            "radio/fm_hiss_loop",
            FRINGE_BED_MAX_VOLUME * depth * depth * ctx.settings.radio_volume,
            600,
        );
        self.fringe_bed_active = true;
        if self.picket_duck_s > 0.0 {
            self.picket_duck_s -= dt;
            if self.picket_duck_s <= 0.0 && self.radio_picket_duck != 1.0 {
                self.radio_picket_duck = 1.0;
                self.apply_radio_volume(ctx);
            }
        }
        if signal >= PICKET_SIGNAL {
            return;
        }
        let picket_depth = (PICKET_SIGNAL - signal) / PICKET_SIGNAL;
        self.picket_wait_s -= dt;
        if self.picket_wait_s > 0.0 {
            return;
        }
        let freq = if self.radio_fringe_freq != 0.0 {
            self.radio_fringe_freq
        } else {
            FM_DEFAULT_MHZ
        };
        let wavelength_m = 299.792458 / freq;
        let mut rate = 2.0 * self.trip.truck.velocity_mps.abs() / wavelength_m;
        rate = rate.clamp(PICKET_MIN_RATE_HZ, PICKET_MAX_RATE_HZ);
        rate *= 0.3 + 0.7 * picket_depth;
        self.picket_wait_s = self.fringe_rng.expovariate(rate);
        // Owner's ear 2026-07-24: pickets sit UNDER the program at shallow
        // fringe (they play on the hotter sfx bus, so numbers here run low)
        // and only rival it deep in the noise.
        ctx.audio.play_bank_with(
            "radio/picket",
            "radio/static_burst",
            (0.15 + 0.35 * picket_depth) * ctx.settings.radio_volume,
            0.0,
        );
        // Honors Settings > Audio the same as every other duck in the game.
        // This one is arguably signal simulation rather than an accessibility
        // step-back -- a picket IS the program dipping under multipath fade --
        // but "do not step my audio back" is one behavior with one name, and a
        // player who turned it off did not mean "except for this". The burst
        // still plays at full level; it just no longer digs itself a hole.
        if ctx.settings.duck_audio_for_speech {
            self.radio_picket_duck = PICKET_DUCK;
            self.picket_duck_s = 0.05 + 0.08 * self.fringe_rng.random();
            self.apply_radio_volume(ctx);
        }
    }

    pub fn stop_radio_fringe(&mut self, ctx: &mut GameContext) {
        if self.fringe_bed_active {
            ctx.audio.stop_loop_with(CH_RADIO_FX, 400);
            self.fringe_bed_active = false;
        }
        if self.radio_picket_duck != 1.0 {
            self.radio_picket_duck = 1.0;
            self.picket_duck_s = 0.0;
            self.apply_radio_volume(ctx);
        }
    }

    /// Every station is on the air for this long, whatever the dial says.
    ///
    /// Called once a frame from whichever update owns the frame -- the drive
    /// itself, or the covered-music tick while a menu is over it -- so the
    /// clock runs at the same rate with the radio off, tuned elsewhere, or
    /// playing. Real seconds, like the rotation it drives.
    pub fn advance_radio_airtime(&mut self, dt: f64) {
        self.radio_airtime_s += dt.max(0.0);
        self.advance_channel3000(dt);
    }

    /// The key that fixes this station's running order for this trip.
    fn station_seed_key(&self, station: &RadioStation) -> String {
        format!("{}|{}", self.trip_seed, station.id)
    }

    /// Where this station's rotation stands right now.
    ///
    /// `radio_airtime_s` opens the drive part way in, so the first tune-in
    /// arrives mid-song the way a real dial does, and keeps running while the
    /// driver is elsewhere on the dial, which is why tuning back finds the
    /// station further along instead of back at the top.
    pub(super) fn station_cue(
        &self,
        ctx: &GameContext,
        station: &RadioStation,
        tracks: &[String],
    ) -> RotationCue {
        if tracks.is_empty() {
            return RotationCue::default();
        }
        let seed_key = self.station_seed_key(station);
        let rotation = StationRotation {
            station_id: &station.id,
            host: &station.host,
            playlist: &station.playlist,
            seed_key: &seed_key,
            tracks,
            breaks: !self.synth_roadhouse(ctx, station),
        };
        cue_after(&rotation, self.radio_airtime_s)
    }

    pub fn station_rotation_pool(
        &self,
        ctx: &GameContext,
        station: &RadioStation,
        night: bool,
    ) -> Vec<String> {
        if let Some(pool) = self.synth_roadhouse_pool(ctx, station, night) {
            return pool;
        }
        if station.playlist == "route" {
            return if night {
                self.night_music_sequence.clone()
            } else {
                self.day_music_sequence.clone()
            };
        }
        if !station.playlist.is_empty() {
            return select_station_playlist(
                &station.playlist,
                &format!("{}|{}", self.trip_seed, station.id),
            );
        }
        if !station.track_key.is_empty() {
            return vec![station.track_key.clone()];
        }
        Vec::new()
    }

    pub fn start_station_rotation(
        &mut self,
        ctx: &mut GameContext,
        station: &RadioStation,
        fade_ms: u32,
    ) {
        // The local clock, as the drive's own flag is: on the trip's Eastern
        // clock the two disagree for hours, and update_radio_playback took
        // every frame of them as night falling and restarted the station.
        let night = self.night_now();
        self.music_night = night;
        self.radio_station_id = station.id.clone();
        self.radio_playlist = self.station_rotation_pool(ctx, station, night);
        self.radio_playing_key.clear();
        self.synth_music_applied = Some(self.roadhouse_synth_state(ctx));
        let cue = self.station_cue(ctx, station, &self.radio_playlist);
        let key = if cue.in_break() {
            cue.current_key(&self.radio_playlist)
        } else {
            self.resolve_station_track(ctx, cue.track_index)
        };
        // A spoken host break, station ID or ad plays whole. Cutting into one
        // mid-word is what a real dial does and what a screen reader user
        // should never have to sit through, and a few seconds of drift on a
        // rotation nobody is timing costs nothing.
        let start_s = if cue.in_break() { 0.0 } else { cue.elapsed_s };
        self.radio_track_index = cue.track_index;
        self.radio_elapsed_s = start_s;
        self.radio_break_queue = cue.break_queue;
        self.radio_break_pos = cue.break_pos;
        self.radio_break_count = cue.break_count;
        self.radio_tracks_since_break = cue.tracks_since_break;
        if !key.is_empty() {
            ctx.audio.play_music_at(&key, fade_ms, start_s);
        }
    }

    pub fn update_radio_playback(&mut self, ctx: &mut GameContext, night: bool, dt: f64) {
        let station = self.radio.current_station();
        if station.real_stream || station.fallback {
            return;
        }
        if Self::is_channel3000(&station) {
            self.update_channel3000_playback(ctx);
            return;
        }
        if station.source_type == PERSONAL_PLAYLIST_SOURCE_TYPE {
            self.update_playlist_playback(ctx, &station, dt);
            return;
        }
        if station.playlist.is_empty() && station.track_key.is_empty() {
            return;
        }
        if station.id != self.radio_station_id
            || (station.playlist == "route" && night != self.music_night)
        {
            self.start_station_rotation(ctx, &station, 2500);
            return;
        }
        if self.radio_playlist.is_empty() {
            return;
        }
        self.radio_elapsed_s += dt.max(0.0);
        let len = if !self.radio_break_queue.is_empty() {
            content_duration_s(&self.radio_break_queue[self.radio_break_pos])
        } else {
            let current = self.current_station_track();
            self.station_track_len_s(ctx, &station, &current)
        };
        if self.radio_elapsed_s < len {
            return;
        }
        self.radio_elapsed_s = 0.0;
        if !self.radio_break_queue.is_empty() {
            self.radio_break_pos += 1;
            if self.radio_break_pos < self.radio_break_queue.len() {
                let next = self.radio_break_queue[self.radio_break_pos].clone();
                ctx.audio.play_music_with(&next, 300);
                return;
            }
            self.radio_break_queue = Vec::new();
            self.play_station_track(ctx, 1200);
            return;
        }
        self.radio_track_index += 1;
        self.radio_tracks_since_break += 1;
        if self.radio_tracks_since_break >= RADIO_TRACKS_PER_HOST_BREAK
            && !self.synth_roadhouse(ctx, &station)
        {
            let queue = plan_break(
                &station.id,
                &station.host,
                &station.playlist,
                &format!("{}|{}", self.trip_seed, station.id),
                self.radio_break_count,
            );
            self.radio_tracks_since_break = 0;
            if !queue.is_empty() {
                let first = queue[0].clone();
                self.radio_break_queue = queue;
                self.radio_break_pos = 0;
                self.radio_break_count += 1;
                ctx.audio.play_music_with(&first, 600);
                return;
            }
        }
        self.play_station_track(ctx, 2500);
    }

    pub fn play_station_track(&mut self, ctx: &mut GameContext, fade_ms: u32) {
        let key = self.resolve_station_track(ctx, self.radio_track_index);
        if !key.is_empty() {
            ctx.audio.play_music_with(&key, fade_ms);
        }
    }

    /// `_start_playlist_station(station, fade_ms=900, advance=False)`, with
    /// the failure the Python version raised.
    ///
    /// A file entry plays off disk; a stream entry tunes the same live
    /// connection the curated real streams use. Entries that will not open
    /// are skipped at play time rather than pruned at load: a NAS that was
    /// asleep when the drive started should not erase the tracks behind it.
    /// Returns `Err` only when nothing in the whole playlist opens, so the
    /// radio's existing fallback machinery can speak the failure the same way
    /// it does a dead stream.
    pub fn start_playlist_station_checked(
        &mut self,
        ctx: &mut GameContext,
        station: &RadioStation,
        fade_ms: u32,
        advance: bool,
    ) -> Result<(), RadioPlaybackError> {
        let entries = station.playlist_entries.clone();
        if entries.is_empty() {
            return Err(RadioPlaybackError("playlist is empty".to_string()));
        }
        let shuffle = ctx.settings.radio_shuffle_playlists && entries.len() > 1;
        let candidates: Vec<usize> = if shuffle {
            self.shuffled_candidates(&station.id, entries.len(), advance)
        } else {
            let mut start = self
                .playlist_positions
                .get(&station.id)
                .copied()
                .unwrap_or(0);
            if advance {
                start = (start + 1) % entries.len();
            }
            (0..entries.len())
                .map(|attempt| (start + attempt) % entries.len())
                .collect()
        };
        for index in candidates {
            let entry = &entries[index];
            let stream = is_stream_entry(entry);
            let played = if stream {
                ctx.audio.play_radio_stream_with(entry, fade_ms)
            } else {
                ctx.audio.play_music_file_with(entry, fade_ms)
            };
            if played.is_err() {
                continue;
            }
            self.playlist_positions.insert(station.id.clone(), index);
            if shuffle {
                if let Some(lap) = self.playlist_shuffle.get_mut(&station.id) {
                    lap.cursor = lap.order.iter().position(|&i| i == index).unwrap_or(0);
                }
            }
            self.radio_station_id = station.id.clone();
            self.radio_playlist = Vec::new();
            self.radio_playing_key.clear();
            self.radio_break_queue = Vec::new();
            // A file's fade-in window would read as "finished" to
            // music_playing on some backends, and a stream has not even
            // connected yet; either way, hold the advance check off.
            self.playlist_wait_s = if stream {
                PLAYLIST_CONNECT_HOLD_S
            } else {
                PLAYLIST_FADE_HOLD_S
            };
            self.playlist_stream_tries = 0;
            return Ok(());
        }
        Err(RadioPlaybackError(
            "no playable entry in this playlist".to_string(),
        ))
    }

    /// The entry indices to try, in order, with shuffle on: the rest of this
    /// lap from its cursor (the next slot when advancing, the current one
    /// when tuning back in), then the slots already played, so a lap of
    /// unreadable files still tries every entry once. A lap that has run
    /// out is replaced first: a fresh permutation, and one that never opens
    /// on the track that just ended.
    ///
    /// Each lap is a Fisher-Yates shuffle on a PyRandom seeded from the trip
    /// seed, the playlist and the lap number, the way every other roll in
    /// the drive is seeded, so a transcript replays and a test can pin it.
    fn shuffled_candidates(&mut self, station_id: &str, len: usize, advance: bool) -> Vec<usize> {
        let last = self.playlist_positions.get(station_id).copied();
        let trip_seed = self.trip_seed;
        let lap = self
            .playlist_shuffle
            .entry(station_id.to_string())
            .or_insert_with(|| PlaylistShuffleLap {
                order: shuffled_lap(trip_seed, station_id, 0, len, None),
                cursor: 0,
                lap: 0,
            });
        if lap.order.len() != len {
            // The file was re-read with a different track count mid-drive.
            *lap = PlaylistShuffleLap {
                order: shuffled_lap(trip_seed, station_id, lap.lap + 1, len, last),
                cursor: 0,
                lap: lap.lap + 1,
            };
            return lap.order.clone();
        }
        let mut cursor = lap.cursor;
        if advance {
            cursor += 1;
            if cursor >= len {
                *lap = PlaylistShuffleLap {
                    order: shuffled_lap(trip_seed, station_id, lap.lap + 1, len, last),
                    cursor: 0,
                    lap: lap.lap + 1,
                };
                return lap.order.clone();
            }
        }
        let mut out: Vec<usize> = lap.order[cursor..].to_vec();
        out.extend_from_slice(&lap.order[..cursor]);
        out
    }

    /// `_start_playlist_station` as the playback backend calls it, where the
    /// Python exception had nowhere to go through the trait's `Result`.
    pub fn start_playlist_station(
        &mut self,
        ctx: &mut GameContext,
        station: &RadioStation,
        fade_ms: u32,
        advance: bool,
    ) {
        let _ = self.start_playlist_station_checked(ctx, station, fade_ms, advance);
    }

    /// Whether the station on the air can report the song it is playing.
    ///
    /// A live stream carries ICY metadata; so does a personal playlist
    /// sitting on a stream entry, because that entry IS an internet station
    /// -- a playlist exported from an internet radio app is nothing but
    /// stations. A playlist on a file off the player's own disk is not a
    /// broadcast and nobody is publishing a title for it.
    pub fn station_sends_song_info(&self, station: &RadioStation) -> bool {
        station.real_stream
            || (station.source_type == PERSONAL_PLAYLIST_SOURCE_TYPE
                && is_stream_entry(&self.playlist_entry(station)))
    }

    /// The entry this playlist is sitting on right now.
    pub fn playlist_entry(&self, station: &RadioStation) -> String {
        let entries = &station.playlist_entries;
        if entries.is_empty() {
            return String::new();
        }
        let index = self
            .playlist_positions
            .get(&station.id)
            .copied()
            .unwrap_or(0);
        entries[index % entries.len()].clone()
    }

    /// Advance a personal playlist when the current entry ends.
    ///
    /// A live stream never ends: it holds the dial until the driver tunes
    /// away or the connection dies. Since play_radio_stream connects on a
    /// worker thread, a silent music channel right after tuning means
    /// "still connecting", never "finished" -- so a stream entry gets a
    /// connect hold and a re-tune before it is written off.
    pub fn update_playlist_playback(
        &mut self,
        ctx: &mut GameContext,
        station: &RadioStation,
        dt: f64,
    ) {
        if station.id != self.radio_station_id {
            self.playlist_wait_s = 0.0;
            self.playlist_stream_skips = 0;
            if self
                .start_playlist_station_checked(ctx, station, 2500, false)
                .is_err()
            {
                self.playlist_nothing_plays(ctx, station);
                self.radio_station_id = station.id.clone();
            }
            return;
        }
        self.playlist_wait_s = (self.playlist_wait_s - dt.max(0.0)).max(0.0);
        if self.playlist_wait_s > 0.0 {
            return;
        }
        if ctx.audio.music_playing() {
            self.playlist_stream_tries = 0;
            self.playlist_stream_skips = 0;
            self.playlist_silence_spoken.remove(&station.id);
            return;
        }
        let entry = self.playlist_entry(station);
        if is_stream_entry(&entry) && self.playlist_stream_tries < PLAYLIST_CONNECT_TRIES {
            // Re-tune the same URL: one still connecting is a no-op, and one
            // whose connect already failed fails here -- which is exactly
            // when the entry gets skipped, the same as an unreadable file.
            self.playlist_stream_tries += 1;
            if ctx
                .audio
                .play_radio_stream_with(&entry, RADIO_TUNE_FADE_MS)
                .is_ok()
            {
                self.playlist_wait_s = PLAYLIST_CONNECT_HOLD_S;
                return;
            }
        }
        if self
            .start_playlist_station_checked(ctx, station, 1200, true)
            .is_err()
        {
            self.playlist_nothing_plays(ctx, station);
            return;
        }
        if is_stream_entry(&self.playlist_entry(station)) {
            // A stream cannot say up front whether it will play, so a
            // playlist of dead streams would otherwise cycle silently
            // forever. One lap of the entries with nothing ever heard is
            // the same answer as nothing opening at all.
            self.playlist_stream_skips += 1;
            if self.playlist_stream_skips as usize > station.playlist_entries.len() {
                self.playlist_nothing_plays(ctx, station);
            }
        } else {
            self.playlist_stream_skips = 0;
        }
    }

    /// Nothing in this playlist would play: say so once, then keep trying.
    ///
    /// Silence with no explanation is the bug this feature kept hitting.
    /// The line names the folder the player can go and fix, and it speaks
    /// once per station until something in it plays again -- a station that
    /// is simply between tracks says nothing at all.
    pub fn playlist_nothing_plays(&mut self, ctx: &mut GameContext, station: &RadioStation) {
        ctx.audio.stop_music_with(600);
        self.playlist_wait_s = PLAYLIST_RETRY_S;
        self.playlist_stream_tries = 0;
        self.playlist_stream_skips = 0;
        if self.playlist_silence_spoken.contains(&station.id) {
            return;
        }
        self.playlist_silence_spoken.insert(station.id.clone());
        ctx.say_event_with(
            format!(
                "Nothing in {} would play. Check the tracks in your Playlists folder.",
                station.display_name()
            ),
            SayEvent::queued().category(SpeechCategory::Status),
        );
    }

    /// Badges for actually living on the dial rather than just switching it on.
    ///
    /// The catalog had nothing for the radio at all, which is a strange gap in
    /// a game with five hundred odd real stations and terrain-aware
    /// propagation: the interesting things a driver notices -- a signal held
    /// across three states, a station arriving from far outside its contour --
    /// went unremarked.
    pub fn track_radio_badges(&mut self, ctx: &mut GameContext, reception: &RadioReception) {
        if ctx.profile.is_none() {
            return;
        }
        let station = &reception.station;
        let heard = add_unique_stat(profile_mut_of(ctx), "radio_stations_heard", &station.id);
        if heard >= 25 {
            ctx.award_driving_achievement("radio_dial_wanderer");
        }
        // A genuine skip: audible past the station's flat contour, which only
        // height can do. Any station merely ridden into its own static must
        // not count -- that is Somewhere in the Static's territory, and this
        // badge used to pop on every ordinary fade-out drive. The flat
        // contour is effective_range_miles with no elevation term, i.e. the
        // reach-doubled contour (RADIO_REACH_MULT), not the raw published
        // range_miles.
        if let Some(distance) = reception.distance_miles {
            if station.range_miles > 0.0 && distance >= effective_range_miles(station, None) * 1.1 {
                ctx.award_driving_achievement("radio_fringe_catch");
            }
        }
        let state = self.trip.state_at(None);
        if state.is_empty() {
            return;
        }
        if self.radio_states_station != station.id {
            self.radio_states_station = station.id.clone();
            self.radio_states_held.clear();
            self.radio_states_held.insert(state);
            return;
        }
        self.radio_states_held.insert(state);
        if self.radio_states_held.len() >= 3 {
            ctx.award_driving_achievement("radio_three_states");
        }
    }

    /// Badges for the driving itself: craft, and one or two bad ideas.
    ///
    /// Kept out of the physics so nothing here can change how the truck
    /// behaves -- these only ever read.
    pub fn track_driving_badges(&mut self, ctx: &mut GameContext, dt: f64) {
        if ctx.profile.is_none() || !self.trip.truck.engine_on {
            return;
        }
        let speed = self.trip.truck.speed_mph();
        // A mile held at exactly sixty-nine. It means nothing. It is also the
        // single most requested number in the history of odometers.
        if (68.5..=69.5).contains(&speed) {
            self.nice_speed_mi += speed * dt / 3600.0;
            if self.nice_speed_mi >= 1.0 {
                ctx.award_driving_achievement("sixty_nine_mph");
            }
        } else {
            self.nice_speed_mi = 0.0;
        }
        // And a mile at the old national maximum, for anyone who learned to
        // drive a truck in a cab that never let them off it. Same shape as
        // the one above: a held number, for a mile, deliberately.
        if (54.5..=55.5).contains(&speed) {
            self.double_nickel_mi += speed * dt / 3600.0;
            if self.double_nickel_mi >= 1.0 {
                ctx.award_driving_achievement("fifty_five_mph");
            }
        } else {
            self.double_nickel_mi = 0.0;
        }
        if speed >= 88.0 {
            ctx.award_driving_achievement("eighty_eight_mph");
        }
        if self.trip.truck.brake_temp_c >= self.trip.truck.brake_fade_onset_c() {
            ctx.award_driving_achievement("brake_smoke");
        }
        // Two miles of real downgrade held on the engine alone. The service
        // brake touching at all resets it -- that is the whole point.
        if self.trip.truck.grade <= -0.04 && self.trip.truck.engine_brake() && speed > 5.0 {
            if self.trip.truck.brake > 0.01 || self.trip.truck.emergency_brake {
                self.jake_descent_mi = 0.0;
            } else {
                self.jake_descent_mi += speed * dt / 3600.0;
                if self.jake_descent_mi >= 2.0 {
                    ctx.award_driving_achievement("jake_only_descent");
                }
            }
        } else if self.trip.truck.grade > -0.02 {
            self.jake_descent_mi = 0.0;
        }
        // Predictive cruise banking speed for a grade that would really have
        // taken it: the feature earning its keep, once, out loud.
        if self.cruise_mph.is_some()
            && self.pcc_phase == "building"
            && self.grade_extremes_ahead().0 >= 0.04
        {
            ctx.award_driving_achievement("predictive_crest");
        }
    }

    /// `self.radio.write_settings(self.ctx.settings); self.ctx.settings.save()`.
    fn write_radio_settings(&mut self, ctx: &mut GameContext) {
        {
            let mut view = RadioSettingsMut(&mut ctx.settings);
            self.radio.write_settings(&mut view);
        }
        let _ = ctx.settings.save();
    }

    pub fn sync_radio_settings(&mut self, ctx: &mut GameContext) {
        let station_before = self.radio.station_id.clone();
        {
            let view = RadioSettingsView(&ctx.settings);
            self.radio.apply_settings(&view);
        }
        let (position, elevation) = self.radio_position(ctx);
        self.radio.update_position(position, elevation);
        self.radio.current_station();
        if self.radio.station_id != station_before {
            self.write_radio_settings(ctx);
        }
    }

    /// `RadioState._station_allowed`, which `ff_core` keeps private: whether
    /// this station may still play under the current streamer-safe setting.
    fn station_allowed(&self, station: &RadioStation) -> bool {
        if !station.supported {
            return false;
        }
        if self.radio.unplayable_ids.contains(&station.id) || !self.radio.synth_allows(station) {
            return false;
        }
        if !station.real_stream && station.source_type != PERSONAL_PLAYLIST_SOURCE_TYPE {
            return true;
        }
        // Real streams and personal media ride the same gate: the game
        // cannot vouch for their licensing, and streamer-safe mode is the
        // one switch that keeps such audio off a broadcast.
        !self.radio.streamer_safe
    }

    /// React to a radio settings flip while this drive owns the radio.
    ///
    /// Turning streamer-safe on is a promise about what is on the air
    /// right now. Before this, the playing stream was never stopped (the
    /// one thing the mode exists to do), the dial swapped to the SILENT
    /// fallback without a word, and flipping the mode back off left the
    /// radio parked on that silence. Now the station leaves the air the
    /// moment the row is toggled, the cab says so, and the radio lands on
    /// the Roadhouse like any other handover.
    pub fn apply_radio_settings_to_drive(&mut self, ctx: &mut GameContext) {
        self.restart_roadhouse_on_synth_change(ctx);
        let before = self.radio.current_station();
        {
            let view = RadioSettingsView(&ctx.settings);
            self.radio.apply_settings(&view);
        }
        if self.station_allowed(&before) {
            return;
        }
        let powered = self.radio.enabled && self.trip.truck.engine_on;
        let action = self.with_radio_backend(ctx, |radio, backend| {
            radio.select_station(
                SAFE_ROUTE_PLAYLIST,
                if powered { Some(backend) } else { None },
            )
        });
        self.write_radio_settings(ctx);
        if powered {
            let reason = if self.radio.streamer_safe {
                "streamer-safe mode is on"
            } else {
                "Music source is Synthesized"
            };
            ctx.say_event_with(
                format!(
                    "{} left the dial, {reason}. Tuned to {}.",
                    before.display_name(),
                    action.station.display_name()
                ),
                SayEvent::queued().category(SpeechCategory::Status),
            );
        }
    }

    /// Music volume = radio setting x signal x ducks.
    pub fn apply_radio_volume(&mut self, ctx: &mut GameContext) {
        let factor = self.radio_signal_factor;
        // A sibling of the picket duck, deliberately not the same field: the
        // picket duck self-heals on stop_radio_fringe, which would drag an
        // enforcement duck away with it in the middle of a cue.
        let duck = self.radio_picket_duck * self.radio_cue_duck;
        ctx.audio
            .set_volumes(&VolumeUpdate::default().music(ctx.settings.radio_volume * factor * duck));
    }

    pub fn play_radio_current(&mut self, ctx: &mut GameContext) {
        self.sync_radio_settings(ctx);
        // An explicit (re)start IS the power sync for this frame; without
        // this, resuming a running-engine trip would restart the song twice.
        self.radio_powered = self.trip.truck.engine_on;
        if self.radio.enabled && self.trip.truck.engine_on {
            self.apply_radio_volume(ctx);
            self.with_radio_backend(ctx, |radio, backend| radio.play(Some(backend), ""));
        } else {
            ctx.audio.stop_music_with(600);
        }
    }

    /// The radio draws power from the engine.
    ///
    /// Every engine path funnels through here on the next frame -- the
    /// ignition key, a stall, a rest-menu shutdown -- so the radio falls
    /// silent with the engine and comes back on its own when the engine
    /// does (owner ruling, 2026-08-12: no radio in a dead cab, starting
    /// with the engine-off top of every load).
    pub fn sync_radio_power(&mut self, ctx: &mut GameContext) {
        let powered = self.trip.truck.engine_on;
        if powered == self.radio_powered {
            return;
        }
        self.radio_powered = powered;
        if !self.radio.enabled {
            return;
        }
        if powered {
            self.play_radio_current(ctx);
        } else {
            ctx.audio.stop_music_with(600);
            self.stop_radio_fringe(ctx);
        }
    }

    pub fn finish_radio_action(&mut self, ctx: &mut GameContext, action: &RadioAction) {
        self.write_radio_settings(ctx);
        // A locked dial's answer is an empty message (station commands do
        // nothing and say nothing while Synthesized streamer-safe mode
        // holds the radio on the Roadhouse) -- never speak it.
        if !action.message.is_empty() {
            ctx.say(&action.message);
        }
    }

    /// Speak the dead-cab line when a radio key lands with no engine.
    pub fn radio_no_power(&mut self, ctx: &mut GameContext) -> bool {
        if self.trip.truck.engine_on {
            return false;
        }
        ctx.audio.play("ui/error");
        ctx.say("The engine is off. The radio has no power.");
        true
    }

    pub fn toggle_radio(&mut self, ctx: &mut GameContext) {
        self.player_touched_radio(ctx);
        if self.radio_no_power(ctx) {
            return;
        }
        self.sync_radio_settings(ctx);
        let action = self.with_radio_backend(ctx, |radio, backend| radio.toggle(Some(backend)));
        self.finish_radio_action(ctx, &action);
    }

    /// Tick and say so when a dial key lands on a radio that is off.
    ///
    /// Sibling of `radio_no_power`, and deliberately the same shape: the
    /// two "not right now" answers should feel identical, so a driver learns
    /// one response rather than two. Call it after the power check -- no
    /// power is the more basic reason and owns the reply.
    pub fn radio_switched_off(&mut self, ctx: &mut GameContext) -> bool {
        if self.radio.enabled {
            return false;
        }
        ctx.audio.play("ui/error");
        ctx.say("Radio off.");
        true
    }

    pub fn tune_radio(&mut self, ctx: &mut GameContext, direction: i64) {
        self.player_touched_radio(ctx);
        if self.radio_no_power(ctx) {
            return;
        }
        self.sync_radio_settings(ctx);
        if self.radio_switched_off(ctx) {
            return;
        }
        let action =
            self.with_radio_backend(ctx, |radio, backend| radio.tune(direction, Some(backend)));
        self.finish_radio_action(ctx, &action);
    }

    pub fn jump_radio_category(&mut self, ctx: &mut GameContext, direction: i64) {
        self.player_touched_radio(ctx);
        if self.radio_no_power(ctx) {
            return;
        }
        self.sync_radio_settings(ctx);
        if self.radio_switched_off(ctx) {
            return;
        }
        let action = self.with_radio_backend(ctx, |radio, backend| {
            radio.tune_category(direction, Some(backend))
        });
        self.finish_radio_action(ctx, &action);
    }

    /// Tune to one station by id from the Radio app; the spoken result.
    ///
    /// Unlike the dial keys this switches a radio that is off back on:
    /// picking a station from a list is asking to hear it. The engine
    /// still has to be running -- the app cannot give the radio power.
    pub fn tune_radio_to(&mut self, ctx: &mut GameContext, station_id: &str) -> String {
        self.player_touched_radio(ctx);
        if !self.trip.truck.engine_on {
            ctx.audio.play("ui/error");
            return "The engine is off. The radio has no power.".to_string();
        }
        self.sync_radio_settings(ctx);
        if self.radio.station_locked() && station_id != SAFE_ROUTE_PLAYLIST {
            return String::new();
        }
        let was_off = !self.radio.enabled;
        self.radio.enabled = true;
        let id = station_id.to_string();
        let action = self.with_radio_backend(ctx, |radio, backend| {
            radio.select_station(&id, Some(backend))
        });
        self.write_radio_settings(ctx);
        let message = action.message;
        if was_off {
            return format!("Radio on. {message}");
        }
        message
    }

    pub fn speak_radio_status(&mut self, ctx: &mut GameContext) {
        self.sync_radio_settings(ctx);
        let mut status = self.radio.status_text();
        if !self.trip.truck.engine_on {
            // "Radio on" over a silent cab contradicts the player's ears;
            // the same explanation the Tab radio screen gives goes here too.
            status = format!("{status} The engine is off. The radio has no power.");
        }
        ctx.say(&status);
    }

    /// One spoken sentence: the song the tuned station reports, or why
    /// there is none. Shared by Shift+Y, the Tab radio screen, and the
    /// Radio app, so the three never disagree.
    pub fn radio_now_playing_text(&mut self, ctx: &mut GameContext) -> String {
        if !self.radio.enabled {
            return "The radio is off.".to_string();
        }
        if !self.trip.truck.engine_on {
            return "The engine is off. The radio has no power.".to_string();
        }
        let station = self.radio.current_station();
        if let Some(text) = self.synth_now_playing(ctx, &station) {
            return text;
        }
        if let Some(text) = self.channel3000_now_playing(&station) {
            return text;
        }
        if !self.station_sends_song_info(&station) {
            return format!("{} does not send song information.", station.display_name());
        }
        if !ctx.audio.music_playing() {
            return format!(
                "{} is still connecting; nothing is playing yet.",
                station.display_name()
            );
        }
        // Re-read rather than trust the tick's copy: the key is the one place
        // a player asks at an exact moment, and a title that changed in the
        // last second is the title they are asking about.
        let title = ctx
            .audio
            .radio_now_playing()
            .or_else(|| self.radio_now_playing.clone());
        let Some(title) = title.filter(|title| !title.is_empty()) else {
            return format!(
                "{} is not sending song information right now.",
                station.display_name()
            );
        };
        self.radio_now_playing = Some(title.clone());
        format!("Now playing on {}: {title}.", station.display_name())
    }

    pub fn speak_radio_now_playing(&mut self, ctx: &mut GameContext) {
        self.sync_radio_settings(ctx);
        let text = self.radio_now_playing_text(ctx);
        ctx.say(&text);
    }

    pub fn toggle_radio_favorite(&mut self, ctx: &mut GameContext) {
        self.sync_radio_settings(ctx);
        let message = self.radio.toggle_favorite();
        // A locked dial answers with an empty message and touches nothing:
        // favorites stay exactly as they were.
        if message.is_empty() {
            return;
        }
        if ctx.profile.is_some() {
            let mut favorites: Vec<String> = self.radio.favorite_ids.iter().cloned().collect();
            favorites.sort();
            profile_mut_of(ctx).radio_favorites = favorites;
            ctx.save_profile();
        }
        ctx.say(&message);
    }

    /// Shift with the dial keys: 10-percent steps on the same grid and
    /// rounding as the Settings > Audio "In-cab radio volume" row, so the
    /// two controls never disagree about a reachable value. Unlike the
    /// plain tune and Ctrl category-jump keys, this works whether the
    /// radio is on or off and whether the engine is running: the setting
    /// is what it is regardless of power state.
    pub fn adjust_radio_volume(&mut self, ctx: &mut GameContext, direction: i64) {
        let value = ff_core::pyfmt::round_py_n(
            ctx.settings.radio_volume + RADIO_VOLUME_STEP * direction as f64,
            2,
        )
        .clamp(0.0, 1.0);
        ctx.settings.radio_volume = value;
        let _ = ctx.settings.save();
        self.apply_radio_volume(ctx);
        let pct = ff_core::pyfmt::round_py(value * 100.0) as i64;
        if pct <= 0 {
            ctx.say("Radio volume muted.");
        } else if pct >= 100 {
            ctx.say("Radio volume all the way up.");
        } else {
            ctx.say(&format!("Radio volume {pct} percent."));
        }
    }
}
