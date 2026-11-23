//! The Hearken backend's truck engine: the multisample ring with per-band
//! rate tracking, the legacy single pitched loop, the ignition handoff,
//! and the per-frame `update`.

use std::rc::Rc;

use ff_core::audio_fades::Fade;
use ff_core::engine_audio::{IgnitionEvent, IgnitionPlayback};
use hearken::audio::{Bus, LoopRegion, Sound, Start};

use super::assets::{asset_bytes, stereo_engine_available};
use super::backend::Category;
use super::hearken_backend::{bus_of, level, EngineBand, HearkenBackend};
use super::hearken_clips::{cab_start_frames, CAB_LOOP_FADE_FRAMES, CAB_SAMPLE_RATE};
use super::{
    advance_engine_wobble, engine_band_weights, engine_band_weights_with_width, engine_freq_mult,
    engine_load_gain, ENGINE_BANDS, ENGINE_BAND_RATE_MAX, ENGINE_BAND_RATE_MIN,
    ENGINE_CLASSIC_LOOP_KEY, ENGINE_LOOP_GAIN, ENGINE_LOOP_KEY, ENGINE_RESUME_FADE_S,
    ENGINE_RPM_IDLE, ENGINE_SLIDE_MS, ENGINE_SLIDE_SNAP_MS, ENGINE_SLIDE_SNAP_RPM, SFX_EXTENSIONS,
    STEREO_ENGINE_BANDS, STEREO_ENGINE_START_KEY,
};

// The recorded cab engine follows the live clutch-out rev fall closely.
// Blend level separately so differently recorded bands enter smoothly.
const STEREO_REV_SLIDE_MS: u32 = 55;
const STEREO_GAIN_SLIDE_MS: u32 = 30;
const IGNITION_HANDOFF_GAIN_MS: u32 = 10;
// The cab takes differ in tone. Blend a little longer while keeping all
// audible cuts inside the rate clamp, even across the widest band gap.
const STEREO_XFADE_LOG_FRAC: f64 = 0.40;

enum EngineLoopEntry {
    AfterIgnition,
    Resume,
}

impl HearkenBackend {
    /// One engine loop, silent, at the engine's pan.
    fn start_engine_loop(&mut self, key: &str) -> Option<hearken::audio::Sound> {
        let start = Start::LOOP.volume(0.0).pan(self.engine_pan as f32);
        self.start_sfx(key, bus_of(Category::Engine), start)
    }

    fn start_engine_bands(&mut self, bands: &[(&str, f64)]) {
        for &(key, native) in bands {
            let Some(sound) = self.start_engine_loop(key) else {
                continue;
            };
            self.engine_bands.push(EngineBand {
                native,
                sound,
                last_rate: 1.0,
                last_volume: 0.0,
            });
        }
    }

    pub(super) fn engine_start(&mut self, play_start_sound: bool) {
        if self.engine_running {
            return;
        }
        self.engine_running = true;
        self.fades.clear();
        self.engine_last_rpm = ENGINE_RPM_IDLE;
        self.engine_last_throttle = 0.0;
        if play_start_sound {
            let bus = bus_of(Category::Engine);
            let start = Start::ONCE.volume(level(&self.buses, Category::Engine, 1.0));
            let mut intro = None;
            if !self.engine_voice_classic && stereo_engine_available() {
                if self.start_stereo_ignition(bus, start) {
                    return;
                }
                if asset_bytes(STEREO_ENGINE_START_KEY, SFX_EXTENSIONS).is_some() {
                    intro = self.start_sfx(STEREO_ENGINE_START_KEY, bus, start);
                }
            }
            if let Some(sound) = intro.or_else(|| self.start_sfx("engine/start", bus, start)) {
                self.engine_ignition = self.engine_ignition.next(IgnitionEvent::PlayOneShot(sound));
                self.engine_intro_gain.set(0.0);
                return;
            }
        }
        self.start_running_engine(EngineLoopEntry::Resume);
    }

    fn start_stereo_ignition(&mut self, bus: Bus, start: Start) -> bool {
        // A deferred device can re-create long-lived loops from their
        // starting positions. Keep that path on the ordinary one-shot.
        if self.opening || self.key_filter.is_some() {
            return false;
        }
        let Some(frames) = asset_bytes(STEREO_ENGINE_START_KEY, SFX_EXTENSIONS)
            .and_then(|(bytes, _)| cab_start_frames(&bytes))
        else {
            return false;
        };
        let loop_start = (frames + CAB_LOOP_FADE_FRAMES) as f64 / f64::from(CAB_SAMPLE_RATE);
        let loop_config = Start::LOOP
            .volume(0.0)
            .pan(self.engine_pan as f32)
            .region(LoopRegion::from(loop_start));
        let keys = STEREO_ENGINE_BANDS.map(|(key, _)| format!("{key}@{frames}"));
        let mut starts = vec![(STEREO_ENGINE_START_KEY, bus, start)];
        starts.extend(keys.iter().map(|key| (key.as_str(), bus, loop_config)));
        // These WAVs carry exactly the starter's length in silence. Start
        // them on the same sample so the handoff happens in the mixer.
        let sounds = self.audio.start_together(&starts);
        if sounds.contains(&Sound::NONE) {
            for sound in sounds {
                self.audio.stop(sound, 0);
            }
            return false;
        }
        if let Some(requested) = self.requested_keys.as_mut() {
            requested.push(STEREO_ENGINE_START_KEY.into());
            requested.extend(STEREO_ENGINE_BANDS.map(|(key, _)| key.to_string()));
        }
        self.engine_ignition = self
            .engine_ignition
            .next(IgnitionEvent::PlayCabStarter(sounds[0]));
        self.engine_bands
            .extend(
                STEREO_ENGINE_BANDS
                    .iter()
                    .zip(&sounds[1..])
                    .map(|(&(_, native), &sound)| EngineBand {
                        native,
                        sound,
                        last_rate: 1.0,
                        last_volume: 0.0,
                    }),
            );
        self.engine_stereo_ring = true;
        self.engine_wobble.resize(self.engine_bands.len(), [0.0; 2]);
        self.engine_intro_gain.set(1.0);
        self.set_engine_rpm(self.engine_last_rpm, self.engine_last_throttle);
        true
    }

    fn start_running_engine(&mut self, entry: EngineLoopEntry) {
        let initial_gain_ms = match entry {
            EngineLoopEntry::AfterIgnition => {
                // Send the running gain this frame. A single audio-thread ramp
                // avoids waiting for a second game update after the crank ends.
                self.engine_intro_gain.set(1.0);
                Some(IGNITION_HANDOFF_GAIN_MS)
            }
            EngineLoopEntry::Resume => {
                let gain = Rc::clone(&self.engine_intro_gain);
                self.fades.add(Fade::new(
                    move |value| gain.set(value),
                    0.0,
                    1.0,
                    ENGINE_RESUME_FADE_S,
                ));
                None
            }
        };
        self.engine_stereo_ring = false;
        if !self.engine_voice_classic {
            if stereo_engine_available() {
                self.start_engine_bands(&STEREO_ENGINE_BANDS);
                self.engine_stereo_ring = self.engine_bands.len() == STEREO_ENGINE_BANDS.len();
                if !self.engine_stereo_ring {
                    for band in self.engine_bands.drain(..) {
                        self.audio.stop(band.sound, 0);
                    }
                }
            }
            if !self.engine_stereo_ring {
                self.start_engine_bands(&ENGINE_BANDS);
            }
        }
        if self.engine_bands.len() < 2 {
            // Not enough cuts for a crossfade ring (a clean clone carries
            // only the synthesized engine/idle): legacy single pitched loop.
            for band in self.engine_bands.drain(..) {
                self.audio.stop(band.sound, 0);
            }
            let mut sound = None;
            if self.engine_voice_classic {
                sound = self.start_engine_loop(ENGINE_CLASSIC_LOOP_KEY);
                if sound.is_none() {
                    log::warn!(
                        "Classic engine cut {ENGINE_CLASSIC_LOOP_KEY} is not in this build; using {ENGINE_LOOP_KEY} pitched instead"
                    );
                }
            }
            if sound.is_none() {
                sound = self.start_engine_loop(ENGINE_LOOP_KEY);
            }
            self.engine_sound = sound;
        }
        self.engine_wobble.resize(self.engine_bands.len(), [0.0; 2]);
        self.apply_engine_rpm(
            self.engine_last_rpm,
            self.engine_last_throttle,
            initial_gain_ms,
        );
    }

    pub(super) fn update(&mut self, dt: f64) {
        // First, so `is_playing` and every handle below answer for this
        // frame.
        self.audio.update(&|_| false);
        self.follow_open();
        self.collect_radio_stream();
        self.fades.update(dt);
        let rpm = self.engine_last_rpm;
        let load = self.engine_last_throttle;
        match self.engine_ignition {
            IgnitionPlayback::OneShot(sound) | IgnitionPlayback::CabStarter(sound)
                if !self.audio.is_playing(sound) =>
            {
                self.engine_ignition = self.engine_ignition.next(IgnitionEvent::StarterFinished);
                if matches!(self.engine_ignition, IgnitionPlayback::CabIdleRise { .. })
                    && !self.engine_bands.is_empty()
                {
                    self.set_engine_rpm(rpm, load);
                } else {
                    // The fallback may also be needed if the device lost a
                    // prepared band while the starter was still running.
                    self.engine_ignition = self.engine_ignition.next(IgnitionEvent::Stop);
                    self.start_running_engine(EngineLoopEntry::AfterIgnition);
                }
            }
            IgnitionPlayback::CabIdleRise { .. } => {
                self.engine_ignition =
                    self.engine_ignition
                        .next(IgnitionEvent::Update { dt, rpm, load });
                self.set_engine_rpm(rpm, load);
            }
            _ if self.engine_running && self.engine_intro_gain.get() != self.intro_applied => {
                self.set_engine_rpm(rpm, load);
            }
            _ => {}
        }
        // Advance the per-band anti-repetition walks; set_engine_rpm
        // applies them.
        advance_engine_wobble(&mut self.engine_wobble, dt, &mut self.wobble_rng);
    }

    pub(super) fn engine_stop(&mut self, shutdown_sound: bool) {
        self.reverse_stop();
        if !self.engine_running {
            return;
        }
        self.engine_running = false;
        self.fades.clear();
        self.engine_intro_gain.set(1.0);
        let intro = self.engine_ignition.starter();
        self.engine_ignition = self.engine_ignition.next(IgnitionEvent::Stop);
        if let Some(sound) = intro {
            self.audio.stop(sound, 0);
        }
        let fade_ms = if shutdown_sound || intro.is_some() {
            0
        } else {
            250
        };
        for band in std::mem::take(&mut self.engine_bands) {
            self.audio.stop(band.sound, fade_ms);
        }
        self.engine_wobble.clear();
        if let Some(sound) = self.engine_sound.take() {
            self.audio.stop(sound, fade_ms);
        }
        if shutdown_sound {
            self.play("engine/shutdown", 1.0, 0.0);
        }
    }

    /// Update only pan; retain it across stops and voice changes.
    pub(super) fn set_engine_pan(&mut self, pan: f64) {
        self.engine_pan = pan.clamp(-1.0, 1.0);
        let sounds: Vec<_> = self
            .engine_bands
            .iter()
            .map(|band| band.sound)
            .chain(self.engine_sound)
            .collect();
        for sound in sounds {
            self.audio.set_pan_over(sound, self.engine_pan as f32, 0);
        }
    }

    /// Track RPM: crossfade the multisample ring, or pitch the legacy loop.
    ///
    /// With the ring, each band's playback rate also slides toward
    /// `rpm / native_rpm` (clamped) so the pitch is continuous through a
    /// crossfade instead of stepping between the cuts' recorded speeds.
    pub(super) fn set_engine_rpm(&mut self, rpm: f64, throttle: f64) {
        self.apply_engine_rpm(rpm, throttle, None);
    }

    fn apply_engine_rpm(&mut self, rpm: f64, throttle: f64, gain_ramp_ms: Option<u32>) {
        if !self.engine_running {
            return;
        }
        // A step-sized rpm change (shift re-entry) snaps; wander glides.
        let slide_ms = if (rpm - self.engine_last_rpm).abs() > ENGINE_SLIDE_SNAP_RPM
            || (self.engine_stereo_ring && self.engine_duck < 1.0)
        {
            ENGINE_SLIDE_SNAP_MS
        } else if self.engine_stereo_ring {
            STEREO_REV_SLIDE_MS
        } else {
            ENGINE_SLIDE_MS
        };
        self.engine_last_rpm = rpm;
        self.engine_last_throttle = throttle;
        self.engine_ignition = self.engine_ignition.next(IgnitionEvent::Update {
            dt: 0.0,
            rpm,
            load: throttle,
        });
        let rpm = self.engine_ignition.audible_rpm(rpm, throttle);
        let intro_gain = self.engine_intro_gain.get();
        self.intro_applied = intro_gain;
        let level = (ENGINE_LOOP_GAIN
            * engine_load_gain(throttle)
            * self.engine_duck
            * self.buses.engine
            * self.buses.speech_duck
            * self.buses.master
            * intro_gain)
            .clamp(0.0, 1.0);
        if !self.engine_bands.is_empty() {
            let gain_ms = gain_ramp_ms.unwrap_or(if self.engine_stereo_ring {
                STEREO_GAIN_SLIDE_MS
            } else {
                0
            });
            let natives: Vec<f64> = self.engine_bands.iter().map(|band| band.native).collect();
            let weights = if self.engine_stereo_ring {
                engine_band_weights_with_width(rpm, &natives, STEREO_XFADE_LOG_FRAC)
            } else {
                engine_band_weights(rpm, &natives)
            };
            let mut failed = false;
            for (i, band) in self.engine_bands.iter_mut().enumerate() {
                if !self.audio.is_playing(band.sound) {
                    failed = true;
                    break;
                }
                let w = weights[i];
                let wob = self.engine_wobble.get(i).copied().unwrap_or([0.0, 0.0]);
                let mut rate =
                    (rpm / band.native).clamp(ENGINE_BAND_RATE_MIN, ENGINE_BAND_RATE_MAX);
                rate *= 1.0 + wob[0];
                let volume = level * w * (1.0 + wob[1]);
                if self.engine_ignition.starter().is_none() {
                    self.audio.set_pitch_over(band.sound, rate as f32, slide_ms);
                    band.last_rate = rate;
                }
                self.audio
                    .set_volume_over(band.sound, volume as f32, gain_ms);
                band.last_volume = volume;
            }
            if failed {
                for band in self.engine_bands.drain(..) {
                    self.audio.stop(band.sound, 0);
                }
            }
            return;
        }
        let Some(sound) = self.engine_sound else {
            return;
        };
        if !self.audio.is_playing(sound) {
            self.engine_sound = None;
            return;
        }
        self.audio
            .set_pitch_over(sound, engine_freq_mult(rpm) as f32, slide_ms);
        self.audio
            .set_volume_over(sound, level as f32, gain_ramp_ms.unwrap_or(0));
    }

    /// Shift-gap disengage: scale the engine bed below the load floor.
    pub(super) fn set_engine_duck(&mut self, duck: f64) {
        let duck = duck.clamp(0.0, 1.0);
        if duck == self.engine_duck {
            return;
        }
        self.engine_duck = duck;
        self.set_engine_rpm(self.engine_last_rpm, self.engine_last_throttle);
    }
}

#[cfg(test)]
mod tests {
    use ff_core::assets_pack::{register_generated_sound, unregister_generated_sound};
    use ff_core::wav::pcm16_wav;
    use hearken::audio::{Call, RecordingBackend};

    use super::*;

    #[test]
    fn stereo_engine_uses_native_rpms_and_tracks_the_unloaded_shift() {
        let source = pcm16_wav(&[512, -256, -512, 256].repeat(100), 2, 48_000);
        for (key, _) in STEREO_ENGINE_BANDS {
            register_generated_sound(key, source.clone(), "wav");
        }
        let (backend, recorder) = RecordingBackend::new();
        let mut engine = HearkenBackend::with_backend(Box::new(backend));
        engine.engine_start(false);
        for (key, _) in STEREO_ENGINE_BANDS {
            unregister_generated_sound(key);
        }
        assert!(engine.engine_stereo_ring);
        assert_eq!(engine.engine_bands.len(), STEREO_ENGINE_BANDS.len());

        let (key, native) = STEREO_ENGINE_BANDS[3];
        let channel = recorder.channel_of(key).expect("stereo upper-mid loop");
        assert_eq!(engine.engine_bands[3].native, native);

        for (rpm, duck, expected_ms) in [
            (native, 1.0, ENGINE_SLIDE_SNAP_MS),
            (native + 50.0, 1.0, STEREO_REV_SLIDE_MS),
            (native - 50.0, 0.65, ENGINE_SLIDE_SNAP_MS),
        ] {
            engine.set_engine_duck(duck);
            let before = recorder.calls().len();
            engine.set_engine_rpm(rpm, 0.5);
            assert!(recorder.calls()[before..].iter().any(|call| matches!(
                call,
                Call::SetMixRamped { channel: id, mix, ramps }
                    if *id == channel && ramps.pitch_ms == expected_ms
                        && (f64::from(mix.pitch) - rpm / native).abs() < 1e-6
            )));
        }
    }
}
