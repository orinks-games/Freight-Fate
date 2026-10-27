//! The Hearken implementation of [`AudioBackend`]: Freight Fate's sounds,
//! music and radio through Hearken's pool of sounds over Kira. It replaced
//! BASS, and everything the facade asks is answered the way BASS answered
//! it:
//!
//! * **Levels are computed here, bit for bit as BASS computed them.** Every
//!   Hearken bus stays at 1.0 and each sound is sent its absolute level,
//!   `clamp(volume * category * master)`, so a bank's level jitter above
//!   unity and the engine's load contour land exactly where they did.
//! * **Shapes are BASS's.** Kira is opened with linear-amplitude fades and
//!   ramps and a balance pan law, a level or pan the game sets is set at
//!   once, and a slide the game asks for (a 120 ms pitch slide, a loop's
//!   fade in) is the one it asked for.
//! * **Clips come from the game's own lookup** ([`FfClips`]), as bytes, and
//!   a missing key plays nothing rather than a stand-in tone.
//! * **The device opens off the game loop** ([`HearkenBackend::deferred`]):
//!   the game starts on Hearken's silent stand-in and the pool swaps the
//!   device in when it opens, carrying every loop across; a music track
//!   started meanwhile is played again from the top.
//!
//! Split over four files: this one (construction, one-shots, loops, the
//! sustain loop, volumes, shutdown, inspection), `hearken_engine` (the
//! engine ring, the ignition crossfade, `update`), `hearken_music` (music
//! tracks and playlist files) and `hearken_radio` (live stations).

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use ff_core::audio_fades::FadeScheduler;
use ff_core::audio_loops::SustainLoopSpec;
use ff_core::pyrandom::PyRandom;
use hearken::audio::{
    Audio as Pool, Backend, Bus, CabSpeakers, CabSpeakersParams, GainCurve, Insert, KiraBackend,
    LoopRegion, PanLaw, SilentBackend, Sound, Start,
};

use super::backend::{loop_category, one_shot_category, Buses, Category, VolumeUpdate};
use super::hearken_clips::{clip_sample_rate, FfClips};
use super::hearken_radio::{RadioConnect, RadioPlaying};
use super::{KeyProbe, RadioVoicing, CH_RADIO_FX, CH_REVERSE, CH_ROAD, ENGINE_RPM_IDLE};

/// The radio's own sounds (its hiss bed, pickets and static) ride this bus
/// so the cab-speaker voicing reaches them along with the station. Like
/// every bus it stays at 1.0: it changes no level.
pub(super) const RADIO_BUS: Bus = Bus("radio_sounds");

/// How much louder the far (right) door speaker plays than Hearken's
/// default, as a gain: 30 percent. A player heard the left speaker as too
/// dominant (via Josh, 2026-10-20). The far speaker still arrives later than
/// the near one and keeps its own room, so the radio still leans to the
/// driver's side, just less.
pub(super) const RADIO_FAR_SPEAKER_LIFT: f32 = 1.3;

/// How much later the far speaker arrives, in ms. Hearken's default is
/// 0.75 ms, about the widest gap between a listener's two ears, so on
/// headphones it pulled the radio almost fully left whatever the far
/// speaker's level (an early side outweighs several dB of level). At 0.3 ms
/// the radio sits near center with a slight lean to the driver.
pub(super) const RADIO_FAR_SPEAKER_DELAY_MS: f32 = 0.3;

/// The cab-speaker voicing the radio plays through: Hearken's truck stereo
/// with the far speaker lifted by [`RADIO_FAR_SPEAKER_LIFT`] and arriving
/// [`RADIO_FAR_SPEAKER_DELAY_MS`] after the near one.
pub(super) fn radio_cab_params() -> CabSpeakersParams {
    let base = CabSpeakersParams::default();
    CabSpeakersParams {
        far_db: base.far_db + 20.0 * RADIO_FAR_SPEAKER_LIFT.log10(),
        far_delay_ms: RADIO_FAR_SPEAKER_DELAY_MS,
        ..base
    }
}

fn cab_amount(voicing: RadioVoicing) -> f32 {
    if voicing.cab_speakers {
        1.0
    } else {
        0.0
    }
}

/// How long flipping Radio sound between cab speakers and clean
/// crossfades.
pub(super) const RADIO_VOICING_SWITCH_MS: u32 = 300;

/// How long the voicing takes to follow the glass from one frame's
/// position to the next: a little over a frame, so the 2 s travel glides.
pub(super) const RADIO_WINDOW_FOLLOW_MS: u32 = 50;

/// The bus a one-shot rides: the radio's own sounds on [`RADIO_BUS`],
/// everything else on its category's.
pub(super) fn one_shot_bus(key: &str, category: Category) -> Bus {
    if key.starts_with("radio/") {
        RADIO_BUS
    } else {
        bus_of(category)
    }
}

/// The bus a category rides. Every bus stays at 1.0 (levels are computed
/// before the pool), so this names sounds for diagnostics; it changes no
/// level.
pub(super) fn bus_of(category: Category) -> Bus {
    match category {
        Category::Engine => Bus("engine"),
        Category::Weather => Bus("weather"),
        Category::Ui => Bus::UI,
        Category::Siren => Bus("siren"),
        Category::Sfx => Bus::EFFECTS,
    }
}

/// `volume * category * master`, clamped, as BASS set it.
pub(super) fn level(buses: &Buses, category: Category, volume: f64) -> f32 {
    (volume * buses.category_volume(category) * buses.master).clamp(0.0, 1.0) as f32
}

/// A loop on a reserved slot: its key, the gain the facade gave it, the
/// sound, and the last playback rate a slide was sent toward.
pub(super) struct LoopEntry {
    pub key: String,
    pub gain: f64,
    pub sound: Sound,
    pub rate: Option<f64>,
}

/// One band of the multisample ring, as BASS kept it, with the playback
/// rate in place of BASS's frequency.
pub(super) struct EngineBand {
    pub native: f64,
    pub sound: Sound,
    pub last_rate: f64,
    pub last_volume: f64,
}

/// A read-only view of one engine band, for tests and diagnostics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HearkenEngineBand {
    pub native: f64,
    pub sound: Sound,
    /// The playback-rate target of the last slide (`rate` x the wobble):
    /// BASS's `last_rate_target` over the cut's own frequency.
    pub last_rate: f64,
    /// The last volume applied to the band.
    pub last_volume: f64,
}

/// The music request to play again when the sound device opens: a track or
/// a file started on the silent stand-in was a one-shot the swap let go.
pub(super) enum MusicReplay {
    Track { track: String, fade_ms: u32 },
    File { path: String, fade_ms: u32 },
}

/// Whether this run asked for no sound (`SDL_AUDIODRIVER=dummy`: headless
/// CI, the playtest harness, the release smoke check).
pub fn headless_requested() -> bool {
    std::env::var("SDL_AUDIODRIVER")
        .map(|v| v.eq_ignore_ascii_case("dummy"))
        .unwrap_or(false)
}

/// Kira on the default output device, shaped like BASS: linear-amplitude
/// fades and ramps, and a balance pan law.
fn open_kira() -> Result<Box<dyn Backend>, String> {
    let kira = KiraBackend::open()?;
    Ok(Box::new(
        kira.with_gain_curve(GainCurve::Linear)
            .with_pan_law(PanLaw::Balance),
    ))
}

pub struct HearkenBackend {
    pub(super) audio: Pool,
    pub(super) buses: Buses,
    pub(super) enabled: bool,
    /// The sound device would not open, so the pool plays through
    /// Hearken's silent backend: audibly no backend at all.
    pub(super) no_device: bool,
    pub(super) loops: HashMap<u32, LoopEntry>,
    pub(super) sustains: HashSet<u32>,
    /// slot -> (key, sound) still ringing out its release tail after a
    /// release, so a repeat press cannot stack a second sound on the tail.
    pub(super) releasing: HashMap<u32, (String, Sound)>,
    pub(super) exclusive_cues: HashMap<String, Sound>,
    pub(super) music_track: Option<String>,
    pub(super) music_sound: Option<Sound>,
    /// The playing track's length, read when it started.
    pub(super) music_length: Option<f64>,
    /// The track or file on the music channel, to play again if the sound
    /// device opens under it.
    pub(super) music_request: Option<MusicReplay>,
    /// The station on the music channel, when it is a station.
    pub(super) radio: Option<RadioPlaying>,
    /// A station still connecting.
    pub(super) radio_connect: Option<RadioConnect>,
    pub(super) radio_failed_url: Option<String>,
    /// The device is still opening behind the silent stand-in.
    pub(super) opening: bool,
    /// A deferred open failed and the facade has not been told.
    pub(super) device_notice: bool,
    pub(super) engine_pan: f64,
    pub(super) engine_running: bool,
    /// The legacy single pitched loop, when that model is running.
    pub(super) engine_sound: Option<Sound>,
    pub(super) engine_bands: Vec<EngineBand>,
    pub(super) engine_voice_classic: bool,
    /// The ignition one-shot, its level before the crossfade, and the
    /// crossfade multiplier the fade writes.
    pub(super) engine_intro_sound: Option<Sound>,
    pub(super) crank_base: f32,
    pub(super) crank_gain: Rc<Cell<f64>>,
    pub(super) crank_applied: f64,
    pub(super) engine_intro_gain: Rc<Cell<f64>>,
    pub(super) engine_intro_load: Rc<Cell<f64>>,
    pub(super) engine_starting: Rc<Cell<bool>>,
    pub(super) intro_applied: (f64, f64),
    pub(super) engine_last_rpm: f64,
    pub(super) engine_last_throttle: f64,
    /// The driver window's lift on the tire loop, 1.0 with it up.
    pub(super) road_lift: f64,
    pub(super) engine_duck: f64,
    pub(super) fades: FadeScheduler,
    pub(super) engine_wobble: Vec<[f64; 2]>,
    pub(super) wobble_rng: PyRandom,
    pub(super) key_filter: Option<KeyProbe>,
    pub(super) requested_keys: Option<Vec<String>>,
    /// The cab-speaker voicing the radio plays through while driving,
    /// shared by every radio sound that carries it.
    pub(super) radio_cab: CabSpeakers,
    /// The voicing last asked for; `None` off the road.
    pub(super) radio_voicing: Option<RadioVoicing>,
}

impl HearkenBackend {
    /// Open the default sound device through Kira, shaped like BASS, on
    /// this thread; the silent backend when `SDL_AUDIODRIVER=dummy` asks for
    /// no sound, or when no device will open (then [`Self::no_device`] says
    /// so).
    pub fn new() -> Self {
        if headless_requested() {
            log::info!("Audio output: Hearken's silent backend, as asked for by this run");
            return Self::with_backend(Box::new(SilentBackend::default()));
        }
        match open_kira() {
            Ok(kira) => Self::with_backend(kira),
            Err(err) => {
                log::warn!("No audio device ({err}); Hearken is running silent");
                let mut backend = Self::with_backend(Box::new(SilentBackend::default()));
                backend.no_device = true;
                backend
            }
        }
    }

    /// Like [`new`](Self::new), but the device opens on its own thread
    /// while the game starts on the silent stand-in, so a driver that hangs
    /// in its open never holds up the boot. Asked-for silence opens nothing.
    pub fn deferred() -> Self {
        if headless_requested() {
            return Self::new();
        }
        Self::deferred_with(open_kira)
    }

    /// [`deferred`](Self::deferred) over any opener: a test's gated
    /// recording backend, or one that fails.
    pub fn deferred_with(
        open: impl FnOnce() -> Result<Box<dyn Backend>, String> + Send + 'static,
    ) -> Self {
        let mut backend = Self::from_pool(Pool::deferred(open, Box::new(FfClips)));
        backend.opening = backend.audio.opening();
        backend
    }

    /// The backend over any Hearken backend: Kira, the silent one, or a
    /// test's recorder.
    pub fn with_backend(backend: Box<dyn Backend>) -> Self {
        Self::from_pool(Pool::new(backend, Box::new(FfClips)))
    }

    fn from_pool(mut audio: Pool) -> Self {
        // Freight Fate is silent where a sound is missing, and registers
        // generated sounds as it runs.
        audio.set_default_fallback(false);
        // The radio's own sounds always carry the voicing; the music
        // channel only while driving (`set_radio_voicing`), so menu music
        // never does.
        let radio_cab = CabSpeakers::new(radio_cab_params());
        audio.set_bus_insert(RADIO_BUS, Some(Insert::CabSpeakers(radio_cab.clone())));
        log::info!("Hearken audio over {}", audio.backend_name());
        Self {
            audio,
            buses: Buses::new(),
            enabled: true,
            no_device: false,
            loops: HashMap::new(),
            sustains: HashSet::new(),
            releasing: HashMap::new(),
            exclusive_cues: HashMap::new(),
            music_track: None,
            music_sound: None,
            music_length: None,
            music_request: None,
            radio: None,
            radio_connect: None,
            radio_failed_url: None,
            opening: false,
            device_notice: false,
            engine_pan: 0.0,
            engine_running: false,
            engine_sound: None,
            engine_bands: Vec::new(),
            engine_voice_classic: false,
            engine_intro_sound: None,
            crank_base: 0.0,
            crank_gain: Rc::new(Cell::new(1.0)),
            crank_applied: 1.0,
            engine_intro_gain: Rc::new(Cell::new(1.0)),
            engine_intro_load: Rc::new(Cell::new(0.0)),
            engine_starting: Rc::new(Cell::new(false)),
            intro_applied: (1.0, 0.0),
            engine_last_rpm: ENGINE_RPM_IDLE,
            engine_last_throttle: 0.0,
            road_lift: 1.0,
            engine_duck: 1.0,
            fades: FadeScheduler::new(),
            engine_wobble: Vec::new(),
            wobble_rng: PyRandom::new_unseeded(),
            key_filter: None,
            requested_keys: None,
            radio_cab,
            radio_voicing: None,
        }
    }

    /// The radio's voicing while driving; `None` takes it off the music
    /// channel so what starts there next (menu music) plays clean. A sound
    /// already playing keeps the voicing it started with, so a station
    /// fading out at the end of a drive fades out as it sounded.
    pub(super) fn set_radio_voicing(&mut self, voicing: Option<RadioVoicing>) {
        let was = self.radio_voicing;
        if voicing == was {
            return;
        }
        self.radio_voicing = voicing;
        let Some(now) = voicing else {
            self.audio.set_bus_insert(Bus::MUSIC, None);
            return;
        };
        if was.is_none() {
            self.audio.set_bus_insert(
                Bus::MUSIC,
                Some(Insert::CabSpeakers(self.radio_cab.clone())),
            );
        }
        match was {
            // A drive's first frame lands at once; a flip of the setting
            // crossfades.
            None => self.radio_cab.set_amount(cab_amount(now), 0),
            Some(before) if before.cab_speakers != now.cab_speakers => self
                .radio_cab
                .set_amount(cab_amount(now), RADIO_VOICING_SWITCH_MS),
            Some(_) => {}
        }
        if was.is_none_or(|before| before.window != now.window) {
            let ramp = if was.is_none() {
                0
            } else {
                RADIO_WINDOW_FOLLOW_MS
            };
            self.radio_cab
                .set_window(now.window.clamp(0.0, 1.0) as f32, ramp);
        }
    }

    /// The radio's cab voicing, for tests and diagnostics.
    pub fn radio_cab(&self) -> &CabSpeakers {
        &self.radio_cab
    }

    /// Whether the sound device would not open (the pool is silent).
    pub fn no_device(&self) -> bool {
        self.no_device
    }

    /// Whether the device is still opening behind the silent stand-in.
    pub fn opening(&self) -> bool {
        self.opening
    }

    /// Follow a deferred open, first thing in `update` after the pool's own:
    /// once the device is in, a music track is played again; once it has
    /// failed, the notice is armed. Hearken's own notice text is never
    /// spoken: the facade's flag drives the game's line.
    pub(super) fn follow_open(&mut self) {
        if !self.opening || self.audio.opening() {
            return;
        }
        self.opening = false;
        if self.audio.take_notice().is_some() {
            self.no_device = true;
            self.device_notice = true;
            return;
        }
        self.replay_music_after_open();
    }

    /// True once, when a deferred open has failed and nobody has said so.
    pub fn take_device_notice(&mut self) -> bool {
        std::mem::take(&mut self.device_notice)
    }

    // -- assets -------------------------------------------------------------

    /// The bytes for one playback of `key`, through the recorder and the
    /// key filter, as BASS's `sfx_stream` asked for them.
    pub(super) fn sfx_bytes(&mut self, key: &str) -> Option<Arc<[u8]>> {
        if let Some(recorder) = self.requested_keys.as_mut() {
            recorder.push(key.to_string());
        }
        if let Some(filter) = &self.key_filter {
            if !filter(key) {
                return None;
            }
        }
        let Some((data, _ext)) = FfClips::sfx(key) else {
            log::warn!("Missing sound: {key}");
            return None;
        };
        Some(data)
    }

    /// Start `key` as `start` says, on `bus`; `None` when there is nothing
    /// to play or it will not play.
    pub(super) fn start_sfx(&mut self, key: &str, bus: Bus, start: Start) -> Option<Sound> {
        let data = self.sfx_bytes(key)?;
        let sound = self.audio.play_clip(key, data, bus, start);
        if sound == Sound::NONE {
            log::warn!("Could not play {key}");
            return None;
        }
        Some(sound)
    }

    // -- one-shots ----------------------------------------------------------

    pub(super) fn play(&mut self, key: &str, volume: f64, pan: f64) {
        self.start_one_shot(key, volume, pan);
    }

    pub(super) fn play_if_idle(&mut self, key: &str, volume: f64, pan: f64) {
        let audio = &self.audio;
        self.exclusive_cues
            .retain(|_, sound| audio.is_playing(*sound));
        if self.exclusive_cues.contains_key(key) {
            self.update_cue(key, volume, pan);
            return;
        }
        if let Some(sound) = self.start_one_shot(key, volume, pan) {
            self.exclusive_cues.insert(key.to_string(), sound);
        }
    }

    pub(super) fn update_cue(&mut self, key: &str, volume: f64, pan: f64) {
        let Some(&sound) = self.exclusive_cues.get(key) else {
            return;
        };
        let level = level(&self.buses, one_shot_category(key), volume);
        self.audio.set_volume_over(sound, level, 0);
        self.audio
            .set_pan_over(sound, pan.clamp(-1.0, 1.0) as f32, 0);
    }

    /// BASS freed the cue's stream, which stopped it at once.
    pub(super) fn stop_cue(&mut self, key: &str) {
        if let Some(sound) = self.exclusive_cues.remove(key) {
            self.audio.stop(sound, 0);
        }
    }

    fn start_one_shot(&mut self, key: &str, volume: f64, pan: f64) -> Option<Sound> {
        let category = one_shot_category(key);
        let start = Start::ONCE
            .volume(level(&self.buses, category, volume))
            .pan(pan.clamp(-1.0, 1.0) as f32);
        self.start_sfx(key, one_shot_bus(key, category), start)
    }

    // -- loops on reserved slots ------------------------------------------------

    pub(super) fn start_loop(&mut self, channel: u32, key: &str, volume: f64, fade_ms: u32) {
        if let Some(current) = self.loops.get(&channel) {
            if current.key == key {
                self.set_loop_volume(channel, volume);
                return;
            }
            self.stop_loop(channel, fade_ms.min(300));
        }
        let bus = if channel == CH_RADIO_FX {
            RADIO_BUS
        } else {
            bus_of(loop_category(channel))
        };
        let Some(sound) = self.start_sfx(key, bus, Start::LOOP.volume(0.0)) else {
            return;
        };
        self.loops.insert(
            channel,
            LoopEntry {
                key: key.to_string(),
                gain: volume,
                sound,
                rate: None,
            },
        );
        self.apply_loop_volume(channel, fade_ms);
    }

    pub(super) fn set_loop_volume(&mut self, channel: u32, volume: f64) {
        if let Some(entry) = self.loops.get_mut(&channel) {
            entry.gain = volume;
            self.apply_loop_volume(channel, 0);
        }
    }

    pub(super) fn set_loop_pan(&mut self, channel: u32, pan: f64) {
        if let Some(entry) = self.loops.get(&channel) {
            self.audio
                .set_pan_over(entry.sound, pan.clamp(-1.0, 1.0) as f32, 0);
        }
    }

    /// Re-pitch a running loop to `rate` times its recorded speed, over the
    /// same short slide BASS used (stepped jumps on a sustained tone are
    /// zipper noise).
    pub(super) fn set_loop_rate(&mut self, channel: u32, rate: f64) {
        let Some(entry) = self.loops.get_mut(&channel) else {
            return;
        };
        let rate = rate.max(0.05);
        entry.rate = Some(rate);
        self.audio.set_pitch_over(entry.sound, rate as f32, 120);
    }

    pub(super) fn stop_loop(&mut self, channel: u32, fade_ms: u32) {
        if let Some((_key, tail)) = self.releasing.remove(&channel) {
            // Cut the ringing-out tail too, if it has not finished.
            self.audio.stop(tail, fade_ms);
        }
        self.sustains.remove(&channel);
        if let Some(entry) = self.loops.remove(&channel) {
            self.audio.stop(entry.sound, fade_ms);
        }
    }

    /// True while `channel` is still ringing out a release tail of `key`.
    fn release_tail_playing(&mut self, channel: u32, key: &str) -> bool {
        let Some((tail_key, sound)) = self.releasing.get(&channel) else {
            return false;
        };
        if !self.audio.is_playing(*sound) {
            self.releasing.remove(&channel);
            return false;
        }
        tail_key == key
    }

    /// Play `key` and loop only the interior region `spec` describes; the
    /// attack plays once, the region repeats until
    /// [`Self::release_sustain_loop`]. A repeat call while the same key is
    /// already sounding on `channel` -- held or ringing out -- is ignored.
    pub(super) fn start_sustain_loop(
        &mut self,
        channel: u32,
        key: &str,
        spec: SustainLoopSpec,
        volume: f64,
    ) {
        let current_key = self.loops.get(&channel).map(|entry| entry.key.clone());
        if current_key.as_deref() == Some(key) && self.sustains.contains(&channel) {
            self.set_loop_volume(channel, volume);
            return;
        }
        if self.release_tail_playing(channel, key) {
            return;
        }
        if current_key.is_some() {
            self.stop_loop(channel, 0);
        }
        let Some(data) = self.sfx_bytes(key) else {
            return;
        };
        let region = match spec.resolve(clip_sample_rate(&data)) {
            Ok(points) => LoopRegion::new(points.start_s, points.end_s),
            Err(err) => {
                log::warn!("Could not set loop points for {key} ({err})");
                return;
            }
        };
        let bus = bus_of(loop_category(channel));
        let start = Start::LOOP.volume(0.0).region(region);
        let sound = self.audio.play_clip(key, data, bus, start);
        if sound == Sound::NONE {
            log::warn!("Could not play {key}");
            return;
        }
        self.releasing.remove(&channel);
        self.loops.insert(
            channel,
            LoopEntry {
                key: key.to_string(),
                gain: volume,
                sound,
                rate: None,
            },
        );
        self.sustains.insert(channel);
        self.apply_loop_volume(channel, 0);
    }

    /// Stop looping `channel` and let its release tail play to the end;
    /// `fade_ms` fades the tail out instead (0 keeps the natural release).
    pub(super) fn release_sustain_loop(&mut self, channel: u32, fade_ms: u32) {
        if !self.sustains.remove(&channel) {
            // No sustain loop here: a plain stop, so callers can use release
            // and stop interchangeably on a channel.
            self.stop_loop(channel, fade_ms);
            return;
        }
        let Some(entry) = self.loops.remove(&channel) else {
            return;
        };
        if fade_ms > 0 {
            self.audio.stop(entry.sound, fade_ms);
        } else {
            self.audio.release(entry.sound, 0);
        }
        self.releasing.insert(channel, (entry.key, entry.sound));
    }

    pub(super) fn reverse_start(&mut self) {
        self.start_loop(CH_REVERSE, "vehicle/reverse", 0.4, 80);
    }

    pub(super) fn reverse_stop(&mut self) {
        self.stop_loop(CH_REVERSE, 80);
    }

    pub(super) fn apply_loop_volume(&mut self, channel: u32, fade_ms: u32) {
        let Some(entry) = self.loops.get(&channel) else {
            return;
        };
        if !self.audio.is_playing(entry.sound) {
            self.loops.remove(&channel);
            self.sustains.remove(&channel);
            return;
        }
        let level = level(&self.buses, loop_category(channel), entry.gain);
        self.audio.set_volume_over(entry.sound, level, fade_ms);
    }

    pub(super) fn set_road_noise(&mut self, speed_mps: f64) {
        let gain = (speed_mps / 30.0).min(1.0);
        if gain < 0.02 {
            self.stop_loop(CH_ROAD, 500);
            return;
        }
        self.start_loop(CH_ROAD, "vehicle/road", gain * self.road_lift, 400);
        let Some(entry) = self.loops.get_mut(&CH_ROAD) else {
            return;
        };
        let rate = 0.4 + 0.9 * (speed_mps / 30.0).min(1.0);
        entry.rate = Some(rate);
        self.audio.set_pitch_over(entry.sound, rate as f32, 120);
    }

    // -- volume control ---------------------------------------------------------

    pub(super) fn set_volumes(&mut self, volumes: &VolumeUpdate) {
        self.buses.apply(volumes);
        let mut channels: Vec<u32> = self.loops.keys().copied().collect();
        channels.sort_unstable();
        for channel in channels {
            self.apply_loop_volume(channel, 0);
        }
        // The engine through the rpm path: it knows the model and keeps the
        // load contour.
        self.set_engine_rpm(self.engine_last_rpm, self.engine_last_throttle);
        if let Some(sound) = self.music_sound {
            let level = self.buses.music_level() as f32;
            self.audio.set_volume_over(sound, level, 0);
        }
    }

    pub(super) fn shutdown(&mut self) {
        self.fades.clear();
        let channels: Vec<u32> = self.loops.keys().copied().collect();
        for channel in channels {
            self.stop_loop(channel, 0);
        }
        self.engine_stop(false);
        self.stop_music(0);
        self.radio_connect = None;
        self.exclusive_cues.clear();
        self.releasing.clear();
        self.audio.shutdown();
        self.enabled = false;
    }

    // -- inspection ------------------------------------------------------------

    /// The pool, for tests and diagnostics.
    pub fn pool(&self) -> &Pool {
        &self.audio
    }

    /// The reserved slots with a loop on them, sorted.
    pub fn loop_channels(&self) -> Vec<u32> {
        let mut channels: Vec<u32> = self.loops.keys().copied().collect();
        channels.sort_unstable();
        channels
    }

    /// The sound of the loop on `channel`.
    pub fn loop_sound(&self, channel: u32) -> Option<Sound> {
        self.loops.get(&channel).map(|entry| entry.sound)
    }

    /// The last playback rate sent to the loop on `channel`, if one was.
    pub fn loop_rate(&self, channel: u32) -> Option<f64> {
        self.loops.get(&channel)?.rate
    }

    /// The slots carrying a held sustain loop, sorted.
    pub fn sustain_channels(&self) -> Vec<u32> {
        let mut channels: Vec<u32> = self.sustains.iter().copied().collect();
        channels.sort_unstable();
        channels
    }

    /// The `(key, sound)` ringing out a release tail on `channel`.
    pub fn releasing_entry(&self, channel: u32) -> Option<(String, Sound)> {
        self.releasing
            .get(&channel)
            .map(|(key, sound)| (key.clone(), *sound))
    }

    /// The sound of a non-overlapping cue.
    pub fn exclusive_cue_sound(&self, key: &str) -> Option<Sound> {
        self.exclusive_cues.get(key).copied()
    }

    /// Fades still running (the ignition crossfade schedules three).
    pub fn fade_count(&self) -> usize {
        self.fades.len()
    }

    pub fn engine_bands(&self) -> Vec<HearkenEngineBand> {
        self.engine_bands
            .iter()
            .map(|band| HearkenEngineBand {
                native: band.native,
                sound: band.sound,
                last_rate: band.last_rate,
                last_volume: band.last_volume,
            })
            .collect()
    }

    /// The legacy single pitched loop, when that model is running.
    pub fn engine_sound(&self) -> Option<Sound> {
        self.engine_sound
    }

    pub fn engine_intro_gain(&self) -> f64 {
        self.engine_intro_gain.get()
    }

    pub fn engine_intro_load(&self) -> f64 {
        self.engine_intro_load.get()
    }

    /// The per-band `[rate walk, gain walk]` pairs.
    pub fn engine_wobble(&self) -> Vec<[f64; 2]> {
        self.engine_wobble.clone()
    }

    /// Seed the anti-repetition walk.
    pub fn set_wobble_rng(&mut self, rng: PyRandom) {
        self.wobble_rng = rng;
    }

    /// The road loop's last playback-rate target: BASS's slide target over
    /// the clip's own frequency.
    pub fn road_noise_rate(&self) -> Option<f64> {
        self.loop_rate(CH_ROAD)
    }

    pub fn music_track(&self) -> Option<&str> {
        self.music_track.as_deref()
    }

    pub fn music_sound(&self) -> Option<Sound> {
        self.music_sound
    }

    /// Make every key `keep` refuses resolve to nothing (test seam).
    pub fn set_key_filter(&mut self, keep: Option<KeyProbe>) {
        self.key_filter = keep;
    }

    /// Start or stop recording every sound key the backend opens.
    pub fn record_requested_keys(&mut self, on: bool) {
        self.requested_keys = if on { Some(Vec::new()) } else { None };
    }

    /// The keys recorded so far (empty when not recording).
    pub fn requested_keys(&self) -> Vec<String> {
        self.requested_keys.clone().unwrap_or_default()
    }
}

impl Default for HearkenBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[path = "hearken_backend_impl.rs"]
mod backend_impl;
