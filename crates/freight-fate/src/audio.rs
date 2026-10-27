//! Runtime audio engine: sound effects, loops, engine audio, and music.
//!
//! Port of `freight_fate/audio.py`. Two interchangeable backends sit behind
//! the [`AudioEngine`] facade:
//!
//! * **Hearken** ([`HearkenBackend`]) -- the real one: Hearken's pool of
//!   sounds over Kira, with live radio through Hearken's streams. The truck
//!   engine is a multisample ring: one real cab loop per rpm band,
//!   crossfaded equal-power with per-band playback-rate tracking. When the
//!   licensed cuts are absent it falls back to the single idle loop pitched
//!   up with RPM. With no audio device (headless CI) it plays through
//!   Hearken's silent backend, so the full code path still runs.
//! * **null** ([`NullBackend`]) -- every primitive a no-op, so game logic
//!   never needs to check for audio availability. The Python build had a
//!   pygame.mixer fallback between the two; the Rust build does not ship
//!   pygame, so `FREIGHT_FATE_AUDIO_BACKEND=pygame` lands here.
//!
//! The facade's public surface is the [`Audio`] trait: what the rest of the
//! game calls, and what a test double implements. Python default arguments
//! became explicit `_with` variants -- `play(key)` is `play_with(key, 1.0,
//! 0.0)`, `stop_loop(channel)` is `stop_loop_with(channel, 300)`, and so on;
//! every short form is a provided method that applies the Python default.
//! `set_volumes(master=.., sfx=..)` takes a [`VolumeUpdate`] whose `None`
//! fields are the omitted keywords.
//!
//! Sound keys are paths relative to the bundled sound library, without
//! extension: `play("ui/menu_select")` plays
//! `freight_fate/assets/sounds/ui/menu_select.ogg` (or its packed twin).

use std::fmt;

use ff_core::pyrandom::PyRandom;

pub mod assets;
mod backend;
pub mod classic_music;
mod engine;
mod hearken_backend;
pub mod hearken_clips;
mod hearken_engine;
mod hearken_music;
mod hearken_radio;
mod null;

pub use assets::{
    asset_bytes, asset_bytes_from, asset_length_s, asset_path, asset_path_in, asset_roots,
    assets_dir, assets_licensed_dir, playback_bytes, verify_sound_assets, AssetBytes,
    MODULE_EXTENSIONS, MUSIC_EXTENSIONS, SFX_EXTENSIONS, SOUND_ASSETS_MISSING,
};
pub use backend::{loop_category, one_shot_category, AudioBackend, Buses, Category, VolumeUpdate};
pub use engine::AudioEngine;
pub use ff_core::assets_pack::{generated_sound_keys, register_generated_sound};
pub use ff_core::audio_loops::{to_seconds, LoopUnits, SustainLoopSpec};
pub use hearken_backend::{headless_requested, HearkenBackend, HearkenEngineBand};
// The device-free decode check, used by the smoke run and the music picker.
pub use hearken::audio::{probe_clip, ClipInfo};
pub use hearken_radio::radio_stream_options;
pub use null::NullBackend;

// Reserved loop slots. The Python pygame backend mapped them onto mixer
// channels; the Hearken backend uses them as keys for its loop table.
pub const CH_ENGINE: [u32; 5] = [0, 1, 2, 3, 4]; // the engine band crossfade ring (pygame only)
pub const CH_ROAD: u32 = 5;
pub const CH_WEATHER: u32 = 6;
pub const CH_WEATHER_B: u32 = 7;
pub const CH_AMBIENT: u32 = 8;
pub const CH_HORN: u32 = 9;
pub const CH_REVERSE: u32 = 10;
pub const CH_AIR: u32 = 11; // compressor charging the tanks below governor release
pub const CH_BRAKE: u32 = 12; // brake-release air bleed: the hiss bed shaped per release
pub const CH_JAKE: u32 = 13; // engine-brake growl: synthesized loop, stage- and rpm-keyed
pub const CH_RADIO_FX: u32 = 14; // FM fringe hiss bed under a thinning station
pub const CH_EDGE: u32 = 15; // edge-boundary ladder loops: clip / strip / shoulder textures
pub const CH_ALERT: u32 = 16; // continuous alert tones: the stop bar's solid zone
pub const CH_SIREN: u32 = 17; // the held enforcement siren, panned and levelled to the cruiser
pub const CH_SCALE: u32 = 18; // weigh-station approach bed, swelling on real seconds
pub const CH_SURGE: u32 = 19; // liquid running in a tank trailer: gated, silent on other freight
pub const CH_LANE_GUIDE: u32 = 20; // optional lane-guide tone, panned by the guide (off by default)
/// One sound per nearby NPC vehicle, set every frame from where it is.
pub const CH_TRAFFIC_SOUNDS: [u32; 3] = [21, 22, 23];
pub const CH_TRAFFIC_BED: u32 = 24; // the freeway's distant traffic, by its real presence
pub const CH_CAB_WIND: u32 = 25; // air rushing past the open driver window, by road speed
pub const CH_CAB_FAN: u32 = 26; // the cab blower at the fan speed the driver chose

// Everything above must be inside the reservation. set_reserved(n) protects
// channels 0..n-1 from find_channel, and this sat at 14 while CH_RADIO_FX,
// CH_EDGE and CH_ALERT were added above it -- so on the pygame fallback a
// burst of one-shots could evict the FM fringe bed, the edge ladder, or the
// stop bar's held tone mid-warning. Guidance a blind driver is steering by
// must never be stealable: keep this one past the last named slot.
pub const RESERVED: u32 = CH_SURGE + 1;
pub const NUM_CHANNELS: u32 = 32;
// (CH_LANE_GUIDE landed at 20 after RESERVED was set to CH_SURGE + 1 = 20; the
// reservation only ever mattered to the pygame mixer, so the value is kept as
// the Python module had it.)
const _: () = assert!(CH_CAB_FAN < NUM_CHANNELS);

/// A held alert tone is a dead man's switch. Its owner re-asserts it every
/// frame through `hold_alert`, and it stops on its own the moment that stops
/// -- a menu opening over the drive, the moment it warned about ending, an
/// owner that lost track of it. A continuous tone in a blind player's
/// headphones must never be able to outlive the thing it is warning about:
/// the stop bar's solid tone once ran until the game was killed (Shane,
/// 2026-08-03).
pub const ALERT_HOLD_TIMEOUT_S: f64 = 0.4;

/// The same dead man's switch, for a cue its owner sounds itself instead of
/// holding on a channel -- a rhythmic tick, a manoeuvre that ends with a
/// click. The owner re-asserts the latch every frame it has, and asks
/// `cue_held` before playing the sound that ends the cue. A driving state
/// that lost the frame to a menu comes back with the latch already lapsed,
/// so the manoeuvre ends in silence instead of clicking off over the pause
/// screen.
pub const CUE_HOLD_TIMEOUT_S: f64 = 0.4;

/// Horn sustain loop points (samples, at the asset's 44100 Hz). The horn is
/// an attack -> sustain -> release sound: play the attack, loop this tuned
/// interior region while the key/button is held, then let the release tail
/// ring out.
pub const HORN_LOOP_START: u32 = 11816;
pub const HORN_LOOP_END: u32 = 12379;
/// The horn's loop region as the sustain-loop machinery takes it.
pub const HORN_LOOP: SustainLoopSpec =
    SustainLoopSpec::samples(HORN_LOOP_START as f64, HORN_LOOP_END as f64);

/// The multisample engine voice: one steady cab loop per band, cut from the
/// real 896 recording at these native rpms. The Hearken backend crossfades the
/// ring with [`engine_band_weights`] and slides each band's playback rate to
/// track rpm inside the band (see `ENGINE_BAND_RATE_*`).
pub const ENGINE_BANDS: [(&str, f64); 5] = [
    ("engine/idle", 680.0),
    ("engine/low", 950.0),
    ("engine/mid", 1150.0),
    ("engine/midhigh", 1425.0),
    ("engine/high", 1900.0),
];

/// Whether `key` is one of the five engine band cuts (`ENGINE_BAND_KEYS`).
pub fn is_engine_band_key(key: &str) -> bool {
    ENGINE_BANDS.iter().any(|(band, _native)| *band == key)
}

/// The five engine band keys, in ring order.
pub fn engine_band_keys() -> Vec<&'static str> {
    ENGINE_BANDS.iter().map(|(key, _native)| *key).collect()
}

// The jake voice A/B: only the 1600 rpm band has a classic alternative -- it
// is the one representative cut kept from the original synthesized jake, not
// a full second ring -- so this is a single key swap, not a per-band table.
pub const JAKE_RECORDED_KEY: &str = "engine/jake_1600";
pub const JAKE_CLASSIC_KEY: &str = "engine/jake_1600_synth";
/// Every recorded jake cut is `engine/jake_<rpm band>`; the classic voice is
/// the one synthesized cut that stands in for all of them. Derived from
/// [`JAKE_RECORDED_KEY`] rather than written out: a bare prefix literal
/// reads as a sound key to the asset-existence sweep, and there is no file
/// by that name.
pub const JAKE_BAND_PREFIX: &str = "engine/jake_";
// Crossfades live in a narrow window around each adjacent pair's GEOMETRIC
// midpoint (log-space), this fraction of the gap wide. Two things follow:
// a cut never plays far from its recorded speed (rate excursions stay under
// ~16 percent -- past that the moving formants smear, the launch-pull
// weirdness the owner heard), and the two members of a blend always track
// the same rpm honestly, so there is no clamped-versus-tracking pitch clash
// (the ~10 Hz "stop-start" beat at 1700-1800 under the old full-gap fade).
pub const ENGINE_XFADE_LOG_FRAC: f64 = 0.30;
// Safety clamp only -- the windows above keep normal tracking well inside.
// 0.78 covers the widest pair's window edge (the 950 cut entering at ~764);
// 1.30 up lets the 1800 cut reach redline (2200/1800 = 1.22).
pub const ENGINE_BAND_RATE_MIN: f64 = 0.78;
pub const ENGINE_BAND_RATE_MAX: f64 = 1.30;

// Legacy engine model: one idle loop, pitched up with RPM. Still the
// fallback when the licensed multisample cuts are absent (a clean clone has
// only the synthesized engine/idle).
pub const ENGINE_LOOP_KEY: &str = "engine/idle";
// The classic voice: the 1.8.x recording under its own key, because the
// licensed overlay owns "engine/idle" -- when the rebuilt cuts are installed
// the shared key IS the rebuilt idle, and the Settings "classic" promise
// (the original engine sound) must not quietly follow it.
pub const ENGINE_CLASSIC_LOOP_KEY: &str = "engine_classic/idle";
pub const ENGINE_RPM_IDLE: f64 = 600.0;
pub const ENGINE_RPM_MAX: f64 = 2200.0;
pub const ENGINE_FREQ_MAX_MULT: f64 = 1.75;
pub const ENGINE_SLIDE_MS: u32 = 120;
// A large rpm jump is a shift re-entry: the engine is ALREADY at the new
// speed when the clutch hooks up, so the voice must step, not glide -- the
// 120 ms slide across a 400 rpm drop reads as a little meow on every shift,
// machine-gunned through a launch (owner's ear, 2026-07-22).
pub const ENGINE_SLIDE_SNAP_MS: u32 = 25;
pub const ENGINE_SLIDE_SNAP_RPM: f64 = 150.0;
pub const ENGINE_LOOP_GAIN: f64 = 1.0;
// Loop-repetition camouflage (owner + outside review, 2026-07-27): even a
// seam-clean 1-2 s loop is recognizable -- the ear locks onto its spectral
// fingerprint recurring at a perfectly fixed period. Each band's playback
// rate and gain take a slow, bounded random walk, so the loop period is
// never exactly the same twice and the recurrence stops landing where the
// ear predicted. Rate stays within ~5 cents (well under the formant-smear
// threshold); gain within ~0.5 dB. Multisample ring only.
pub const ENGINE_WOBBLE_RATE_MAX: f64 = 0.003; // +/- fraction of playback rate (~5 cents)
pub const ENGINE_WOBBLE_RATE_STEP: f64 = 0.004; // random-walk speed, fraction per second
pub const ENGINE_WOBBLE_GAIN_MAX: f64 = 0.06; // +/- fraction of band gain (~0.5 dB)
pub const ENGINE_WOBBLE_GAIN_STEP: f64 = 0.10;

// Ignition crossfade. When the engine is deliberately started, the
// "engine/start" one-shot plays at full volume while the idle loop is held
// silent; near the tail of the clip the two crossfade over
// ENGINE_START_CROSSFADE_S seconds. Tune these to taste -- curve names are
// keys into `audio_fades::CURVES` (linear, ease_in, ease_out, ease_in_out,
// exponential, equal_power_in/out).
pub const ENGINE_START_CROSSFADE_S: f64 = 0.3; // length of the tail blend
pub const ENGINE_START_TAIL_ANCHOR: bool = true; // blend ends at the clip's end; false = blend from t=0
pub const ENGINE_START_FADE_OUT_CURVE: &str = "equal_power_out"; // start.ogg 1.0 -> 0.0
pub const ENGINE_START_FADE_IN_CURVE: &str = "equal_power_in"; // engine loop 0.0 -> 1.0
pub const ENGINE_START_ASSUMED_LEN_S: f64 = 2.0; // fallback if the clip length can't be queried
                                                 // Short fade-in for a silent (no-crank) engine loop start, e.g. resuming a
                                                 // trip whose engine was already running, or coming back from an in-trip menu.
pub const ENGINE_RESUME_FADE_S: f64 = 0.25;
// After the crank hands off, the loop starts at the crank's (full-load)
// volume so there is no dip, then eases down to its true off-throttle load
// over this window.
pub const ENGINE_START_SETTLE_S: f64 = 0.6; // ease from crank level down to idle load
pub const ENGINE_START_SETTLE_CURVE: &str = "ease_out"; // key into audio_fades.CURVES

/// How far the ducked channels (engine, weather, and the music slot the
/// radio rides) drop while the event voice speaks, when the Settings > Audio
/// option is on: half volume, the modest broadcast-style duck -- the road
/// stays present, the words win (XAG 105; speech priority research, R13).
pub const SPEECH_DUCK_LEVEL: f64 = 0.5;

// Radio streaming. A station connects on its own thread (Hearken's stream
// worker), never inside a frame; these bound how long it may take. (The
// off-thread connect is the pattern from PR #150 by CatalystForChaos.)
// Thirty seconds, not the eight this started at: small Icecast stations
// behind a home connection (Darren Duff radio, 2026-08-22) can take longer
// than that to answer, and at eight the radio wrote them off as dead and
// handed over while they were still coming. The price is a longer silence on
// a station that really is gone.
pub const RADIO_CONNECT_TIMEOUT_MS: u32 = 30000; // give up on a station that will not answer
pub const RADIO_READ_TIMEOUT_MS: u32 = 30000; // and on one that answers then stalls
/// Audio held for a station. Five seconds is useful for unreliable links but
/// makes a tune feel much slower than a player that begins with a small
/// buffer.
pub const RADIO_NET_BUFFER_MS: u32 = 1500;
/// Percentage of [`RADIO_NET_BUFFER_MS`] buffered before playback. Twenty
/// percent is a 300 ms tune-in cushion; the remaining buffer can fill while
/// the stream plays.
pub const RADIO_NET_PREBUF_PERCENT: u32 = 20;
/// Player-facing live streams use a short ramp only to avoid a click. Network
/// buffering, rather than a long cosmetic fade, decides when sound begins.
pub const RADIO_TUNE_FADE_MS: u32 = 250;

/// A predicate over sound keys: the test seam that stands in for "does
/// this key resolve" (`AudioEngine::set_asset_probe`,
/// `HearkenBackend::set_key_filter`).
pub type KeyProbe = Box<dyn Fn(&str) -> bool>;

/// How the radio sounds while driving: through the cab's door speakers or
/// clean, and where the driver's glass is. The cab voicing moves with the
/// glass, so it glides over the window's own travel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RadioVoicing {
    /// The cab's speakers (the default) instead of the clean signal.
    pub cab_speakers: bool,
    /// The driver window, 0 up to 1 down.
    pub window: f64,
}

/// What the Python facade raised as `RuntimeError`: the radio layer reads the
/// message to skip a playlist entry or hand a dead station over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioError {
    pub message: String,
}

impl AudioError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AudioError {}

/// The song a Shoutcast/Icecast stream says it is playing, or None.
///
/// ICY metadata arrives as `StreamTitle='Artist - Title';StreamUrl='';` in
/// whatever bytes the station felt like sending -- UTF-8 from most Icecast
/// mounts, Latin-1 from older Shoutcast ones -- so it is decoded leniently
/// and only the title field is kept. Empty, missing, or whitespace-only
/// titles are None, not "", so callers can say "no song information" instead
/// of reading out nothing.
pub fn parse_icy_stream_title(raw: Option<&[u8]>) -> Option<String> {
    let raw = raw?;
    let text = match std::str::from_utf8(raw) {
        Ok(text) => text.to_owned(),
        // Python: decode("latin-1", errors="replace") -- latin-1 never fails.
        Err(_) => raw.iter().map(|&b| b as char).collect(),
    };
    parse_icy_stream_title_text(&text)
}

/// [`parse_icy_stream_title`] for a block already decoded to text.
pub fn parse_icy_stream_title_text(text: &str) -> Option<String> {
    // re.search(r"StreamTitle='(.*?)';", text, re.S): the first
    // "StreamTitle='" and the first "';" after it, newlines included.
    const OPEN: &str = "StreamTitle='";
    const CLOSE: &str = "';";
    let start = text.find(OPEN)? + OPEN.len();
    let rest = &text[start..];
    let end = rest.find(CLOSE)?;
    spoken_stream_title(&rest[..end])
}

/// The song an Ogg, Opus or FLAC stream names in its comment block, or None.
///
/// Those streams carry no ICY block: the title rides in the codec's own
/// `TITLE=` and `ARTIST=` comments, keys in whatever case the encoder chose.
/// Joined as "Artist - Title", the shape an ICY station sends, so every
/// station reads the same way.
pub fn parse_ogg_stream_title(comments: &[String]) -> Option<String> {
    let field = |key: &str| {
        comments.iter().find_map(|comment| {
            let (name, value) = comment.split_once('=')?;
            let value = value.trim();
            (name.trim().eq_ignore_ascii_case(key) && !value.is_empty()).then_some(value)
        })
    };
    let title = field("title")?;
    match field("artist") {
        Some(artist) => spoken_stream_title(&format!("{artist} - {title}")),
        None => spoken_stream_title(title),
    }
}

/// The song an HLS station names on its current segment, or None.
///
/// The stream hands back the `#EXTINF` line after the colon:
/// `duration,title`.
/// Most stations leave the title empty; "no desc" is an encoder's placeholder
/// for the same thing.
pub fn parse_hls_stream_title(extinf: &str) -> Option<String> {
    let (_duration, title) = extinf.split_once(',')?;
    spoken_stream_title(title).filter(|title| !title.eq_ignore_ascii_case("no desc"))
}

/// A station's raw title as one speakable line, or None when it is blank.
///
/// Network encoders pack ad-insertion fields into the title slot, in two
/// shapes: `title="Song",artist="Band",url="..."`, and
/// `Band - text="Song" song_spot="M" ...`. Only the song survives; anything
/// else is whitespace-collapsed and kept as sent.
fn spoken_stream_title(raw: &str) -> Option<String> {
    let title = if let Some(title) = quoted_field(raw, "title") {
        // An empty song field between tracks is "no information", and the
        // fields around it are never worth reading out.
        if title.is_empty() {
            return None;
        }
        match quoted_field(raw, "artist").filter(|artist| !artist.is_empty()) {
            Some(artist) => format!("{artist} - {title}"),
            None => title.to_string(),
        }
    } else if let Some(text) = quoted_field(raw, "text") {
        if text.is_empty() {
            return None;
        }
        let lead = raw.find("text=\"").map_or("", |at| &raw[..at]);
        format!("{lead}{text}")
    } else {
        raw.to_string()
    };
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    (!title.is_empty()).then_some(title)
}

/// The value of `key="..."` in an encoder's field list, when the key stands
/// on its own (so `title` never matches inside `subtitle`).
fn quoted_field<'a>(raw: &'a str, key: &str) -> Option<&'a str> {
    let open = format!("{key}=\"");
    let at = raw.match_indices(&open).map(|(at, _)| at).find(|&at| {
        raw[..at]
            .chars()
            .next_back()
            .is_none_or(|before| !before.is_alphanumeric())
    })?;
    let value = &raw[at + open.len()..];
    Some(value[..value.find('"')?].trim())
}

/// Playback-frequency multiplier for the legacy engine loop at `rpm`.
///
/// Linear from 1.0 at idle (600 RPM) to 1.75x at redline (2200 RPM), clamped
/// at both ends.
pub fn engine_freq_mult(rpm: f64) -> f64 {
    let t = (rpm - ENGINE_RPM_IDLE) / (ENGINE_RPM_MAX - ENGINE_RPM_IDLE);
    (1.0 + t * (ENGINE_FREQ_MAX_MULT - 1.0)).clamp(1.0, ENGINE_FREQ_MAX_MULT)
}

/// Crossfade weights for the engine band ring at `rpm`.
///
/// Below the first native rpm the first band carries alone, above the last
/// the last does. Between neighbours, one band carries alone until the rpm
/// enters the pair's narrow log-space window around their geometric midpoint
/// (ENGINE_XFADE_LOG_FRAC of the gap); inside it the pair blends equal-power
/// (the loops are uncorrelated recordings, so cos/sin keeps the summed level
/// flat). The pure zones either side of each window are what keep every
/// sounding cut close to its recorded speed.
pub fn engine_band_weights(rpm: f64, natives: &[f64]) -> Vec<f64> {
    let n = natives.len();
    let mut weights = vec![0.0; n];
    if n == 0 {
        return weights;
    }
    if rpm <= natives[0] {
        weights[0] = 1.0;
    } else if rpm >= natives[n - 1] {
        weights[n - 1] = 1.0;
    } else {
        for i in 0..n - 1 {
            if rpm <= natives[i + 1] {
                // Position within the gap in log space, remapped through the
                // centered window: below it the lower band is pure, above it
                // the upper band is pure.
                let t = (rpm / natives[i]).ln() / (natives[i + 1] / natives[i]).ln();
                let half = ENGINE_XFADE_LOG_FRAC / 2.0;
                let s = (t - (0.5 - half)) / ENGINE_XFADE_LOG_FRAC;
                if s <= 0.0 {
                    weights[i] = 1.0;
                } else if s >= 1.0 {
                    weights[i + 1] = 1.0;
                } else {
                    weights[i] = (s * std::f64::consts::PI / 2.0).cos();
                    weights[i + 1] = (s * std::f64::consts::PI / 2.0).sin();
                }
                break;
            }
        }
    }
    weights
}

// Facility docks: big-room interiors get the warehouse loop, yards the gate.
const WAREHOUSE_FACILITY_TYPES: [&str; 4] =
    ["warehouse", "dry_warehouse", "cold_storage", "distribution"];

pub fn facility_ambient_key(facility_type: &str) -> &'static str {
    if WAREHOUSE_FACILITY_TYPES.contains(&facility_type) {
        "ambient/warehouse"
    } else {
        "poi/facility_gate"
    }
}

/// Audible engine effort: present off-throttle, fuller under power.
///
/// The load carries real feedback -- a truck holding speed uphill sits on
/// more throttle and sounds fuller, and an automatic shift briefly unloads
/// the engine. Both stay audible here. The floor sits at 0.68 (not 0.55) so
/// coasting is not too quiet, while the 0.32 span keeps the load contour
/// clearly perceptible. Pumping from accelerator release and adaptive-cruise
/// corrections is handled upstream by smoothing the throttle before it
/// reaches this envelope, not by flattening the range.
pub fn engine_load_gain(throttle: f64) -> f64 {
    0.68 + 0.32 * throttle.clamp(0.0, 1.0)
}

/// Advance the per-band anti-repetition walks by `dt` seconds.
///
/// Each entry is `[rate walk, gain walk]`. Diffusion scales with `sqrt(dt)`
/// so the walk speed is frame-rate independent; the clamp keeps each walk
/// meandering inside its box. The Hearken backend calls this from `update` and
/// applies the walks in `set_engine_rpm`; it is public so the walk can be
/// pinned without a device.
pub fn advance_engine_wobble(wobble: &mut [[f64; 2]], dt: f64, rng: &mut PyRandom) {
    if wobble.is_empty() || dt <= 0.0 {
        return;
    }
    let scale = dt.sqrt();
    for wob in wobble.iter_mut() {
        for (i, (step, bound)) in [
            (ENGINE_WOBBLE_RATE_STEP, ENGINE_WOBBLE_RATE_MAX),
            (ENGINE_WOBBLE_GAIN_STEP, ENGINE_WOBBLE_GAIN_MAX),
        ]
        .into_iter()
        .enumerate()
        {
            wob[i] += rng.uniform(-step, step) * scale;
            wob[i] = wob[i].clamp(-bound, bound);
        }
    }
}

/// The `AudioEngine` facade surface: what the rest of the game calls.
///
/// Every method mirrors the Python facade by name. A Python parameter with
/// a default became an explicit argument on the `_with` form, with the short
/// form provided here applying that default:
///
/// | Python | Rust |
/// |---|---|
/// | `play(key, volume=1.0, pan=0.0)` | `play(key)` / `play_with(key, volume, pan)` |
/// | `play_bank(base, fallback, volume=1.0, pan=0.0)` | `play_bank(base, fallback)` / `play_bank_with(..)` |
/// | `start_loop(channel, key, volume=1.0, fade_ms=300)` | `start_loop(channel, key)` / `start_loop_with(..)` |
/// | `stop_loop(channel, fade_ms=300)` | `stop_loop(channel)` / `stop_loop_with(channel, fade_ms)` |
/// | `start_sustain_loop(channel, key, start, end, *, units="samples", volume=1.0)` | `start_sustain_loop(channel, key, spec)` / `start_sustain_loop_with(channel, key, spec, volume)` -- `spec` carries the points and units |
/// | `release_sustain_loop(channel, fade_ms=0)` | `release_sustain_loop(channel)` / `_with(channel, fade_ms)` |
/// | `hold_alert(key, volume=1.0, fade_ms=60)` | `hold_alert(key)` / `hold_alert_with(key, volume, fade_ms)` |
/// | `release_alert(fade_ms=120)` | `release_alert()` / `release_alert_with(fade_ms)` |
/// | `engine_start(play_start_sound=True)` | `engine_start()` / `engine_start_with(play_start_sound)` |
/// | `engine_stop(shutdown_sound=True)` | `engine_stop()` / `engine_stop_with(shutdown_sound)` |
/// | `set_engine_rpm(rpm, throttle=0.0)` | `set_engine_rpm(rpm)` / `set_engine_rpm_with(rpm, throttle)` |
/// | `set_weather(key, intensity=1.0)` | `set_weather(key)` / `set_weather_with(key, intensity)` |
/// | `set_ambient(key, volume=1.0)` | `set_ambient(key)` / `set_ambient_with(key, volume)` |
/// | `play_music(track, fade_ms=1500)` | `play_music(track)` / `play_music_with(track, fade_ms)` |
/// | `play_radio_stream(url, fade_ms=1500)` raises | `play_radio_stream(url)` / `_with(url, fade_ms)` -> `Result` |
/// | `play_music_file(path, fade_ms=1200)` raises | `play_music_file(path)` / `_with(path, fade_ms)` -> `Result` |
/// | `stop_music(fade_ms=1000)` | `stop_music()` / `stop_music_with(fade_ms)` |
/// | `set_volumes(master=None, ..., siren=None)` | `set_volumes(&VolumeUpdate)` |
/// | properties | `enabled()`, `backend_name()`, `*_volume()`, `engine_running()`, `engine_starting()` |
/// Whether `key` is one of the game's own cues rather than the road's or the
/// truck's: what [`Audio::set_cues_muted`] holds back.
pub fn is_game_cue(key: &str) -> bool {
    key.starts_with("ui/") || key.starts_with("events/")
}

pub trait Audio {
    // -- properties -----------------------------------------------------------
    fn enabled(&self) -> bool;
    fn backend_name(&self) -> &str;
    /// Whether the player still needs telling that this run has no sound,
    /// clearing the flag as it answers.
    ///
    /// True once, and only when the run wanted sound and the device would not
    /// open. Silence that was asked for -- headless, a test double, a
    /// caller-built engine -- answers false, so nothing announces the obvious.
    fn take_silence_notice(&mut self) -> bool {
        false
    }
    fn master_volume(&self) -> f64;
    fn sfx_volume(&self) -> f64;
    fn music_volume(&self) -> f64;
    fn weather_volume(&self) -> f64;
    fn engine_volume(&self) -> f64;
    fn ui_volume(&self) -> f64;
    fn engine_running(&self) -> bool;
    /// True while a deliberate ignition is still crossfading into the loop.
    fn engine_starting(&self) -> bool;

    /// Hold back the game's own cues -- the `ui/` and `events/` one-shots
    /// that announce a menu, a choice or a spoken line -- while `muted`.
    /// The road, the truck and the radio still sound. Set while the
    /// co-driver drives and the player has not pressed anything
    /// (`GameContext::sync_cue_mute`), so none of what the co-driver does
    /// at a stop or on the road beeps at a player who heard no words.
    fn set_cues_muted(&mut self, _muted: bool) {}

    // -- one-shots and loops --------------------------------------------------
    /// The key this one will really sound as, after the jake A/B.
    fn voice_key(&self, key: &str) -> String;
    /// Play a one-shot. `pan` -1.0 = full left, 0 = center, 1.0 = right.
    fn play_with(&mut self, key: &str, volume: f64, pan: f64);
    /// Repeat a cue only after its previous playback has finished.
    fn play_if_idle(&mut self, key: &str, volume: f64, pan: f64) {
        self.play_with(key, volume, pan);
    }
    /// Refresh a playing cue's position without restarting its recording.
    /// Call each frame; release_cue(key) stops it, and an absent owner lapses.
    fn update_cue(&mut self, key: &str, _volume: f64, _pan: f64) {
        self.hold_cue(key);
    }
    fn play(&mut self, key: &str) {
        self.play_with(key, 1.0, 0.0);
    }
    /// Play one cut from a round-robin bank, or `fallback` if none exist.
    fn play_bank_with(&mut self, base: &str, fallback: &str, volume: f64, pan: f64);
    fn play_bank(&mut self, base: &str, fallback: &str) {
        self.play_bank_with(base, fallback, 1.0, 0.0);
    }
    fn set_engine_duck(&mut self, duck: f64);
    fn set_speech_duck(&mut self, duck: f64);
    fn set_engine_voice(&mut self, classic: bool);
    fn set_jake_voice(&mut self, classic: bool);
    /// Whether a sound key resolves (pack, licensed overlay, or loose).
    fn has_asset(&mut self, key: &str) -> bool;
    fn start_loop_with(&mut self, channel: u32, key: &str, volume: f64, fade_ms: u32);
    fn start_loop(&mut self, channel: u32, key: &str) {
        self.start_loop_with(channel, key, 1.0, 300);
    }
    fn set_loop_volume(&mut self, channel: u32, volume: f64);
    fn set_loop_pan(&mut self, channel: u32, pan: f64);
    /// Playback-rate multiplier for a running loop: 1.0 is as recorded,
    /// 0.75 a quarter lower in pitch. A backend without pitch control (the
    /// null one, the fakes) ignores it.
    fn set_loop_rate(&mut self, _channel: u32, _rate: f64) {}
    fn stop_loop_with(&mut self, channel: u32, fade_ms: u32);
    fn stop_loop(&mut self, channel: u32) {
        self.stop_loop_with(channel, 300);
    }
    /// Loop only the interior region of `key` described by `spec`.
    fn start_sustain_loop_with(
        &mut self,
        channel: u32,
        key: &str,
        spec: SustainLoopSpec,
        volume: f64,
    );
    fn start_sustain_loop(&mut self, channel: u32, key: &str, spec: SustainLoopSpec) {
        self.start_sustain_loop_with(channel, key, spec, 1.0);
    }
    /// Stop looping `channel` and let its release tail play to the end.
    fn release_sustain_loop_with(&mut self, channel: u32, fade_ms: u32);
    fn release_sustain_loop(&mut self, channel: u32) {
        self.release_sustain_loop_with(channel, 0);
    }

    // -- held alert tones and held cues -----------------------------------------
    /// Sound the continuous alert tone `key` for the next moment only.
    fn hold_alert_with(&mut self, key: &str, volume: f64, fade_ms: u32);
    fn hold_alert(&mut self, key: &str) {
        self.hold_alert_with(key, 1.0, 60);
    }
    /// Stop a held alert tone now, rather than waiting for it to lapse.
    fn release_alert_with(&mut self, fade_ms: u32);
    fn release_alert(&mut self) {
        self.release_alert_with(120);
    }
    fn hold_cue(&mut self, name: &str);
    fn cue_held(&self, name: &str) -> bool;
    fn release_cue(&mut self, name: &str);

    // -- truck engine -----------------------------------------------------------
    fn engine_start_with(&mut self, play_start_sound: bool);
    fn engine_start(&mut self) {
        self.engine_start_with(true);
    }
    fn engine_stop_with(&mut self, shutdown_sound: bool);
    fn engine_stop(&mut self) {
        self.engine_stop_with(true);
    }
    /// Advance time-based audio fades. Call once per frame from the main loop.
    fn update(&mut self, dt: f64);
    /// Engine position: -1 left, 0 centered, +1 right, independent of RPM/load.
    fn set_engine_pan(&mut self, _pan: f64) {}
    fn set_engine_rpm_with(&mut self, rpm: f64, throttle: f64);
    fn set_engine_rpm(&mut self, rpm: f64) {
        self.set_engine_rpm_with(rpm, 0.0);
    }

    // -- road / weather / ambience ------------------------------------------------
    fn set_road_noise(&mut self, speed_mps: f64);
    fn set_weather_with(&mut self, key: Option<&str>, intensity: f64);
    fn set_weather(&mut self, key: Option<&str>) {
        self.set_weather_with(key, 1.0);
    }
    fn set_wind(&mut self, intensity: f64);
    /// How much louder the outside sounds (the tires, rain, the weather
    /// wind) are than with the cab sealed: 1.0 with the driver window up.
    fn set_outside_lift(&mut self, _lift: f64) {}
    /// The radio's voicing while driving; `None` once the drive is over, so
    /// music started after it (menu music) plays clean. Only the radio is
    /// voiced: the station, its music tracks, and its own hiss and static.
    fn set_radio_voicing(&mut self, _voicing: Option<RadioVoicing>) {}
    fn set_ambient_with(&mut self, key: Option<&str>, volume: f64);
    fn set_ambient(&mut self, key: Option<&str>) {
        self.set_ambient_with(key, 1.0);
    }
    fn horn_start(&mut self);
    fn horn_stop(&mut self);
    fn reverse_start(&mut self);
    fn reverse_stop(&mut self);
    /// Stop engine, road, weather, ambience, held cues, and any held alert tone
    /// (leaving UI sfx alone).
    fn stop_world(&mut self);

    // -- music ------------------------------------------------------------------
    fn play_music_with(&mut self, track: &str, fade_ms: u32);
    fn play_music(&mut self, track: &str) {
        self.play_music_with(track, 1500);
    }
    /// Start a music track `start_s` seconds in.
    ///
    /// Tuning into a station that has been on the air a while lands part way
    /// through whatever it is playing, the way a real dial does. A backend
    /// that cannot seek falls back to the top of the track rather than
    /// refusing to play it.
    fn play_music_at(&mut self, track: &str, fade_ms: u32, start_s: f64) {
        let _ = start_s;
        self.play_music_with(track, fade_ms);
    }
    fn play_radio_stream_with(&mut self, url: &str, fade_ms: u32) -> Result<(), AudioError>;
    fn play_radio_stream(&mut self, url: &str) -> Result<(), AudioError> {
        self.play_radio_stream_with(url, 1500)
    }
    fn play_music_file_with(&mut self, path: &str, fade_ms: u32) -> Result<(), AudioError>;
    fn play_music_file(&mut self, path: &str) -> Result<(), AudioError> {
        self.play_music_file_with(path, 1200)
    }
    /// Whether the music channel is still producing sound.
    fn music_playing(&self) -> bool;
    /// How long the playing music track runs, when the stream can tell.
    fn music_length_s(&self) -> Option<f64> {
        None
    }
    /// The song the playing radio stream reports, or None when it reports
    /// nothing (or nothing is streaming).
    fn radio_now_playing(&self) -> Option<String>;
    fn stop_music_with(&mut self, fade_ms: u32);
    fn stop_music(&mut self) {
        self.stop_music_with(1000);
    }

    // -- volume control -----------------------------------------------------------
    fn set_volumes(&mut self, volumes: &VolumeUpdate);
    fn shutdown(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icy_title_parsing_decodes_utf8_and_latin1() {
        assert_eq!(
            parse_icy_stream_title(Some(b"StreamTitle='Usher - U Remind Me';StreamUrl='';")),
            Some("Usher - U Remind Me".to_string())
        );
        assert_eq!(
            parse_icy_stream_title(Some(b"StreamTitle='Caf\xe9 del Mar';")),
            Some("Caf\u{e9} del Mar".to_string())
        );
        assert_eq!(
            parse_icy_stream_title(Some(b"StreamTitle='  Artist   -   Title  ';")),
            Some("Artist - Title".to_string())
        );
        assert_eq!(parse_icy_stream_title(Some(b"StreamTitle='';")), None);
        assert_eq!(parse_icy_stream_title(None), None);
        // Non-greedy across a newline, as re.S allowed.
        assert_eq!(
            parse_icy_stream_title_text("StreamTitle='a\nb';StreamTitle='c';"),
            Some("a b".to_string())
        );
    }

    #[test]
    fn radio_tune_policy_has_a_short_bounded_start() {
        assert_eq!(RADIO_NET_BUFFER_MS, 1500);
        assert_eq!(RADIO_NET_PREBUF_PERCENT, 20);
        assert_eq!(RADIO_NET_BUFFER_MS * RADIO_NET_PREBUF_PERCENT / 100, 300);
        assert_eq!(RADIO_TUNE_FADE_MS, 250);
    }

    #[test]
    fn jake_band_prefix_is_derived_from_the_recorded_key() {
        let (prefix, _) = JAKE_RECORDED_KEY.rsplit_once('_').unwrap();
        assert_eq!(format!("{prefix}_"), JAKE_BAND_PREFIX);
    }

    #[test]
    fn reservation_covers_every_named_slot() {
        assert_eq!(RESERVED, 20);
    }

    #[test]
    fn wobble_walks_stay_inside_their_boxes() {
        let mut rng = PyRandom::new_from_i64(7);
        let mut wobble = vec![[0.0, 0.0]];
        for _ in 0..120 {
            advance_engine_wobble(&mut wobble, 1.0 / 60.0, &mut rng);
        }
        let [rate, gain] = wobble[0];
        assert!(rate != 0.0 && rate.abs() <= ENGINE_WOBBLE_RATE_MAX);
        assert!(gain != 0.0 && gain.abs() <= ENGINE_WOBBLE_GAIN_MAX);
        // Nothing moves with no time.
        let before = wobble.clone();
        advance_engine_wobble(&mut wobble, 0.0, &mut rng);
        assert_eq!(before, wobble);
    }

    #[test]
    fn facility_ambient_key_routes_big_rooms_to_the_warehouse_loop() {
        assert_eq!(facility_ambient_key("warehouse"), "ambient/warehouse");
        assert_eq!(facility_ambient_key("cold_storage"), "ambient/warehouse");
        assert_eq!(facility_ambient_key("port"), "poi/facility_gate");
    }
}
