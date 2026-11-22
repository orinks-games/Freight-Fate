//! The Hearken backend's truck engine: the multisample ring with per-band
//! rate tracking, the legacy single pitched loop, the ignition crossfade,
//! and the per-frame `update` -- ported line for line from the BASS
//! backend it replaced, with a playback rate where BASS slid a frequency.

use std::rc::Rc;

use ff_core::audio_fades::{curve, Fade};
use hearken::audio::Start;

use super::assets::{asset_length_s, stereo_engine_available};
use super::backend::Category;
use super::hearken_backend::{bus_of, level, EngineBand, HearkenBackend};
use super::{
    advance_engine_wobble, engine_band_weights, engine_freq_mult, engine_load_gain, ENGINE_BANDS,
    ENGINE_BAND_RATE_MAX, ENGINE_BAND_RATE_MIN, ENGINE_CLASSIC_LOOP_KEY, ENGINE_LOOP_GAIN,
    ENGINE_LOOP_KEY, ENGINE_RESUME_FADE_S, ENGINE_RPM_IDLE, ENGINE_SLIDE_MS, ENGINE_SLIDE_SNAP_MS,
    ENGINE_SLIDE_SNAP_RPM, ENGINE_START_ASSUMED_LEN_S, ENGINE_START_CROSSFADE_S,
    ENGINE_START_FADE_IN_CURVE, ENGINE_START_FADE_OUT_CURVE, ENGINE_START_SETTLE_CURVE,
    ENGINE_START_SETTLE_S, ENGINE_START_TAIL_ANCHOR, STEREO_ENGINE_BANDS,
};

// The recorded cab engine follows the live clutch-out rev fall closely.
// Blend level separately so differently recorded bands enter smoothly.
const STEREO_REV_SLIDE_MS: u32 = 55;
const STEREO_GAIN_SLIDE_MS: u32 = 30;

impl HearkenBackend {
    /// A fade that drives the engine loop's intro gain (0 = silent under the
    /// crank, 1 = full), presenting `start` at once.
    fn intro_gain_fade(
        &self,
        start: f64,
        end: f64,
        duration_s: f64,
        curve_name: &str,
        delay_s: f64,
        end_starting: bool,
    ) -> Fade {
        let gain = Rc::clone(&self.engine_intro_gain);
        let on_done: Option<Box<dyn FnMut()>> = if end_starting {
            let starting = Rc::clone(&self.engine_starting);
            Some(Box::new(move || starting.set(false)))
        } else {
            None
        };
        Fade::with_options(
            move |value| gain.set(value.clamp(0.0, 1.0)),
            start,
            end,
            duration_s,
            curve(curve_name),
            delay_s,
            on_done,
        )
    }

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
        // Hold the loop silent while the ignition one-shot plays; crossfade
        // it up at the tail. A silent (resume) start skips the crank.
        self.engine_intro_gain.set(0.0);
        self.engine_intro_load.set(0.0);
        if play_start_sound {
            self.begin_engine_start_crossfade();
        } else {
            let fade = self.intro_gain_fade(
                0.0,
                1.0,
                ENGINE_RESUME_FADE_S,
                ENGINE_START_FADE_IN_CURVE,
                0.0,
                false,
            );
            self.fades.add(fade);
        }
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
        self.set_engine_rpm(ENGINE_RPM_IDLE, 0.0);
    }

    /// Play `engine/start` at full volume and blend into the loop at its
    /// tail.
    fn begin_engine_start_crossfade(&mut self) {
        self.engine_starting.set(true);
        let base = level(&self.buses, Category::Engine, 1.0);
        let start = Start::ONCE.volume(base);
        let Some(sound) = self.start_sfx("engine/start", bus_of(Category::Engine), start) else {
            // No crank available: bring the loop up promptly so the engine
            // is still audible.
            let fade = self.intro_gain_fade(0.0, 1.0, ENGINE_RESUME_FADE_S, "linear", 0.0, true);
            self.fades.add(fade);
            return;
        };
        self.engine_intro_sound = Some(sound);
        self.crank_base = base;
        self.crank_gain.set(1.0);
        self.crank_applied = 1.0;
        let clip_len = self.crank_length_s(sound);
        let delay = if ENGINE_START_TAIL_ANCHOR {
            (clip_len - ENGINE_START_CROSSFADE_S).max(0.0)
        } else {
            0.0
        };
        // Boost the loop to full (crank) load through the handoff so it
        // meets the crank tail at the same level instead of the quieter
        // off-throttle idle.
        self.engine_intro_load.set(1.0);

        // The crank itself: the fade writes the multiplier and `update`
        // applies it, since a closure cannot borrow the pool it lives
        // beside.
        let crank = Rc::clone(&self.crank_gain);
        self.fades.add(Fade::with_options(
            move |m| crank.set(m),
            1.0,
            0.0,
            ENGINE_START_CROSSFADE_S,
            curve(ENGINE_START_FADE_OUT_CURVE),
            delay,
            None,
        ));
        let fade_in = self.intro_gain_fade(
            0.0,
            1.0,
            ENGINE_START_CROSSFADE_S,
            ENGINE_START_FADE_IN_CURVE,
            delay,
            true,
        );
        self.fades.add(fade_in);
        // Once the crossfade completes, ease the load boost back off so the
        // loop settles to its real off-throttle volume.
        let load = Rc::clone(&self.engine_intro_load);
        self.fades.add(Fade::with_options(
            move |value| load.set(value.clamp(0.0, 1.0)),
            1.0,
            0.0,
            ENGINE_START_SETTLE_S,
            curve(ENGINE_START_SETTLE_CURVE),
            delay + ENGINE_START_CROSSFADE_S,
            None,
        ));
    }

    /// The crank's length: what the output decoded, else the clip's own
    /// headers, else a safe fallback.
    fn crank_length_s(&mut self, sound: hearken::audio::Sound) -> f64 {
        self.audio
            .length_s(sound)
            .filter(|s| *s > 0.0)
            .or_else(|| Some(asset_length_s("engine/start")).filter(|s| *s > 0.0))
            .unwrap_or(ENGINE_START_ASSUMED_LEN_S)
    }

    pub(super) fn update(&mut self, dt: f64) {
        // First, so `is_playing` and every handle below answer for this
        // frame.
        self.audio.update(&|_| false);
        self.follow_open();
        self.collect_radio_stream();
        self.fades.update(dt);
        let crank = self.crank_gain.get();
        if crank != self.crank_applied {
            self.crank_applied = crank;
            if let Some(sound) = self.engine_intro_sound {
                // A dying sound ignores it, like BASS's suppressed error.
                let volume = self.crank_base * crank as f32;
                self.audio.set_volume_over(sound, volume, 0);
            }
        }
        // The ignition fades moved the intro gain or load boost: re-apply
        // the engine level at the last known rpm.
        let intro = (self.engine_intro_gain.get(), self.engine_intro_load.get());
        if self.engine_running && intro != self.intro_applied {
            self.set_engine_rpm(self.engine_last_rpm, self.engine_last_throttle);
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
        self.engine_intro_load.set(0.0);
        self.engine_starting.set(false);
        self.engine_intro_sound = None;
        self.crank_gain.set(1.0);
        self.crank_applied = 1.0;
        for band in std::mem::take(&mut self.engine_bands) {
            self.audio.stop(band.sound, 250);
        }
        self.engine_wobble.clear();
        if let Some(sound) = self.engine_sound.take() {
            self.audio.stop(sound, 250);
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
        if !(self.engine_running && (!self.engine_bands.is_empty() || self.engine_sound.is_some()))
        {
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
        let intro_gain = self.engine_intro_gain.get();
        let intro_load = self.engine_intro_load.get();
        self.intro_applied = (intro_gain, intro_load);
        let mut load_gain = engine_load_gain(throttle);
        // During the ignition handoff, boost load toward full so the loop
        // meets the crank tail; the boost eases back to 0 afterward.
        load_gain += intro_load * (1.0 - load_gain);
        let level = (ENGINE_LOOP_GAIN
            * load_gain
            * self.engine_duck
            * self.buses.engine
            * self.buses.speech_duck
            * self.buses.master
            * intro_gain)
            .clamp(0.0, 1.0);
        if !self.engine_bands.is_empty() {
            let gain_ms = if self.engine_stereo_ring {
                STEREO_GAIN_SLIDE_MS
            } else {
                0
            };
            let natives: Vec<f64> = self.engine_bands.iter().map(|band| band.native).collect();
            let weights = engine_band_weights(rpm, &natives);
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
                self.audio.set_pitch_over(band.sound, rate as f32, slide_ms);
                self.audio
                    .set_volume_over(band.sound, volume as f32, gain_ms);
                band.last_rate = rate;
                band.last_volume = volume;
            }
            if failed {
                self.engine_bands.clear();
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
        self.audio.set_volume_over(sound, level as f32, 0);
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
