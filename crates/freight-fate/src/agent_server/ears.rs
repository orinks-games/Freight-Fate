use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::app::App;
use crate::audio::{Audio, AudioError, SustainLoopSpec, VolumeUpdate, CH_ROAD};
use crate::speech::SpeechSink;

const MAX_SOUND_LINES: usize = 150;
// -- ears -----------------------------------------------------------------------------

/// Everything audible, formatted at record time, drained by `listen`.
#[derive(Default)]
pub struct Ears {
    pub(super) lines: Vec<String>,
    /// Engine pitch is set every frame; an ear notices where it went, not
    /// sixty samples a second of it. Cleared at each listen.
    engine_rpm: Vec<f64>,
    road_noise_mps: Option<f64>,
    /// The alert currently held, so a per-frame re-assert is heard once.
    held_alert: Option<String>,
    /// Where the engine and the road bed sit in the stereo field right now.
    /// The pan lines above report a lean only when it moves a step, so after
    /// a quiet stretch an agent steering by ear did not know where it stood
    /// -- a driver hears the pan continuously -- and steered off old news:
    /// with lane keeping off, even in lockstep, the truck swung lane to lane
    /// and left the pavement (agent drive, 2026-09-23).
    engine_lean: i32,
    road_bed_lean: Option<i32>,
}

pub type SharedEars = Rc<RefCell<Ears>>;

impl Ears {
    pub fn shared() -> SharedEars {
        Rc::new(RefCell::new(Ears::default()))
    }
}

fn pan_text(pan: f64) -> &'static str {
    if pan < -0.15 {
        " (left)"
    } else if pan > 0.15 {
        " (right)"
    } else {
        ""
    }
}

/// A continuous pan quantised into the steps an agent should report.
///
/// A one-shot says its side once and is done, but the road bed and the engine
/// are panned EVERY FRAME, and together they are the whole steering instrument
/// with lane keeping off: the engine leans toward where the wheel should go,
/// and the bed sits where the truck is in its lane. Reporting either raw would
/// bury the transcript; reporting neither -- which is what this did until
/// 2026-09-18 -- left an agent deaf to the one channel it was asked to test.
/// Quantised to quarters, a slewing guide reports about as often as a player
/// notices it move.
fn pan_step(pan: f64) -> i32 {
    (pan.clamp(-1.0, 1.0) * 4.0).round() as i32
}

/// How a reported pan step reads.
fn pan_step_text(step: i32) -> String {
    match step {
        0 => "centred".to_string(),
        s if s < 0 => format!("left {}", -s),
        s => format!("right {s}"),
    }
}

// -- the speech tee -------------------------------------------------------------------

/// How an interrupting driving-channel line starts in the ears: the cab
/// cutting in with something to answer now (`wait_for` stops on it).
pub(super) const CAB_CUT_IN: &str = "[spoken:event] (interrupting) ";

/// Passes every call to the real sink (the words still reach the screen
/// reader) while recording what was said.
struct TeeSpeech {
    inner: Box<dyn SpeechSink>,
    ears: SharedEars,
}

impl SpeechSink for TeeSpeech {
    fn say(&mut self, text: &str, interrupt: bool) {
        let cut = if interrupt { " (interrupting)" } else { "" };
        self.ears
            .borrow_mut()
            .lines
            .push(format!("[spoken]{cut} {text}"));
        self.inner.say(text, interrupt);
    }
    fn say_event(&mut self, text: &str, interrupt: bool) {
        let cut = if interrupt { " (interrupting)" } else { "" };
        self.ears
            .borrow_mut()
            .lines
            .push(format!("[spoken:event]{cut} {text}"));
        self.inner.say_event(text, interrupt);
    }
    fn stop_main(&mut self) {
        self.inner.stop_main();
    }
    fn stop_event(&mut self) {
        self.inner.stop_event();
    }
    fn stop(&mut self) {
        self.inner.stop();
    }
    fn poll(&mut self, dt: f64) {
        self.inner.poll(dt);
    }
    fn request_refresh(&mut self) {
        self.inner.request_refresh();
    }
    fn available(&self) -> bool {
        self.inner.available()
    }
    fn backend_name(&self) -> String {
        self.inner.backend_name()
    }
    fn has_separate_event_voice(&self) -> bool {
        self.inner.has_separate_event_voice()
    }
    fn event_backend_name(&self) -> String {
        self.inner.event_backend_name()
    }
    fn supports_rate(&self) -> bool {
        self.inner.supports_rate()
    }
    fn supports_pitch(&self) -> bool {
        self.inner.supports_pitch()
    }
    fn supports_volume(&self) -> bool {
        self.inner.supports_volume()
    }
    fn event_supports_rate(&self) -> bool {
        self.inner.event_supports_rate()
    }
    fn event_backend_options(&self) -> Vec<String> {
        self.inner.event_backend_options()
    }
    fn select_event_backend(&mut self, name: Option<&str>) {
        self.inner.select_event_backend(name);
    }
    fn set_braille_only(&mut self, on: bool) {
        self.inner.set_braille_only(on);
    }
    fn supports_braille(&self) -> bool {
        self.inner.supports_braille()
    }
    fn voice_names(&self) -> Vec<String> {
        self.inner.voice_names()
    }
    fn configure(
        &mut self,
        rate: Option<f64>,
        pitch: Option<f64>,
        volume: Option<f64>,
        voice: Option<&str>,
    ) {
        self.inner.configure(rate, pitch, volume, voice);
    }
    fn say_adjustment_preview(&mut self, setting: &str, text: &str, interrupt: bool) -> bool {
        self.inner.say_adjustment_preview(setting, text, interrupt)
    }
    fn refresh(&mut self) -> bool {
        self.inner.refresh()
    }
    fn shutdown(&mut self) {
        self.inner.shutdown();
    }
    fn shutdown_pumping(&mut self, pump: &mut dyn FnMut()) {
        self.inner.shutdown_pumping(pump);
    }
}

// -- the audio tee --------------------------------------------------------------------

/// Passes every call to the real engine (the sounds still play) while
/// recording the audible facts an agent's ears should carry.
struct TeeAudio {
    inner: Box<dyn Audio>,
    ears: SharedEars,
    weather_key: Option<String>,
    ambient_key: Option<String>,
    loop_keys: HashMap<u32, String>,
    /// Last reported pan step per loop channel, and for the engine, so a pan
    /// held steady says nothing and a pan on the move says so once per step.
    loop_pan_steps: HashMap<u32, i32>,
    engine_pan_step: i32,
}

/// The key the road bed plays under. The game never starts that bed by key:
/// it calls `set_road_noise(speed)`, and the BASS backend starts and stops the
/// `vehicle/road` loop on [`CH_ROAD`] behind that call. The tee has to know
/// the same thing, or the bed's pan reports as "channel 3" (review S1).
const ROAD_BED_KEY: &str = "vehicle/road";
/// Below this the backend stops the road bed rather than playing it at a
/// whisper: `gain = speed / 30`, stopped under 0.02 (audio/bass.rs).
const ROAD_BED_MIN_MPS: f64 = 0.02 * 30.0;

impl TeeAudio {
    fn hear(&self, line: String) {
        self.ears.borrow_mut().lines.push(line);
    }

    /// A loop is now playing `key` on `channel`: said once per start, not
    /// once per frame it is re-asserted.
    fn loop_started(&mut self, channel: u32, key: &str) {
        if self.loop_keys.get(&channel).map(String::as_str) != Some(key) {
            self.hear(format!("[sound bed] {key} starts"));
            self.loop_keys.insert(channel, key.to_string());
        }
    }

    /// Nothing plays on `channel` any more. The pan step goes with the key:
    /// a bed that stops leaning left and restarts centred used to say
    /// nothing, because the tee still remembered "left" for the channel.
    fn loop_stopped(&mut self, channel: u32) {
        self.loop_keys.remove(&channel);
        self.loop_pan_steps.remove(&channel);
        if channel == CH_ROAD {
            self.ears.borrow_mut().road_bed_lean = None;
        }
    }
}

impl Audio for TeeAudio {
    fn enabled(&self) -> bool {
        self.inner.enabled()
    }
    fn backend_name(&self) -> &str {
        self.inner.backend_name()
    }
    fn take_silence_notice(&mut self) -> bool {
        self.inner.take_silence_notice()
    }
    fn master_volume(&self) -> f64 {
        self.inner.master_volume()
    }
    fn sfx_volume(&self) -> f64 {
        self.inner.sfx_volume()
    }
    fn music_volume(&self) -> f64 {
        self.inner.music_volume()
    }
    fn weather_volume(&self) -> f64 {
        self.inner.weather_volume()
    }
    fn engine_volume(&self) -> f64 {
        self.inner.engine_volume()
    }
    fn ui_volume(&self) -> f64 {
        self.inner.ui_volume()
    }
    fn engine_running(&self) -> bool {
        self.inner.engine_running()
    }
    fn engine_starting(&self) -> bool {
        self.inner.engine_starting()
    }
    fn voice_key(&self, key: &str) -> String {
        self.inner.voice_key(key)
    }
    fn play_with(&mut self, key: &str, volume: f64, pan: f64) {
        let soft = if volume < 0.4 { ", soft" } else { "" };
        self.hear(format!("[sound] {key}{}{soft}", pan_text(pan)));
        self.inner.play_with(key, volume, pan);
    }
    fn play_bank_with(&mut self, base: &str, fallback: &str, volume: f64, pan: f64) {
        let soft = if volume < 0.4 { ", soft" } else { "" };
        self.hear(format!("[sound] {base}{}{soft}", pan_text(pan)));
        self.inner.play_bank_with(base, fallback, volume, pan);
    }
    // The held-cue pair reaches the engine as itself. Left to the trait's
    // defaults, a repeat-when-idle came through as a plain play, so in the
    // agent server every blinker click started a fresh copy over the one
    // still sounding (owner, 2026-10-01: the blinker "played twice over each
    // other"), and a held cue's pan never followed the move.
    fn play_if_idle(&mut self, key: &str, volume: f64, pan: f64) {
        let soft = if volume < 0.4 { ", soft" } else { "" };
        self.hear(format!("[sound] {key}{}{soft}", pan_text(pan)));
        self.hold_cue(key);
        self.inner.play_if_idle(key, volume, pan);
    }
    fn update_cue(&mut self, key: &str, volume: f64, pan: f64) {
        self.hold_cue(key);
        self.inner.update_cue(key, volume, pan);
    }
    fn set_engine_duck(&mut self, duck: f64) {
        self.inner.set_engine_duck(duck);
    }
    fn set_speech_duck(&mut self, duck: f64) {
        self.inner.set_speech_duck(duck);
    }
    fn set_engine_voice(&mut self, classic: bool) {
        self.inner.set_engine_voice(classic);
    }
    fn set_jake_voice(&mut self, classic: bool) {
        self.inner.set_jake_voice(classic);
    }
    fn has_asset(&mut self, key: &str) -> bool {
        self.inner.has_asset(key)
    }
    fn start_loop_with(&mut self, channel: u32, key: &str, volume: f64, fade_ms: u32) {
        self.loop_started(channel, key);
        self.inner.start_loop_with(channel, key, volume, fade_ms);
    }
    fn set_loop_volume(&mut self, channel: u32, volume: f64) {
        self.inner.set_loop_volume(channel, volume);
    }
    fn set_loop_pan(&mut self, channel: u32, pan: f64) {
        // A pan on a channel with nothing playing is inaudible, so it is not
        // reported -- the lane-position pan is written to the road bed's
        // channel before the truck is rolling, and "channel 3 pans right"
        // with no bed to hear it was noise in the listen.
        let Some(key) = self.loop_keys.get(&channel).cloned() else {
            self.inner.set_loop_pan(channel, pan);
            return;
        };
        let step = pan_step(pan);
        if self.loop_pan_steps.insert(channel, step) != Some(step) {
            self.hear(format!("[bed] {key} pans {}", pan_step_text(step)));
        }
        if channel == CH_ROAD {
            self.ears.borrow_mut().road_bed_lean = Some(step);
        }
        self.inner.set_loop_pan(channel, pan);
    }

    fn set_engine_pan(&mut self, pan: f64) {
        let step = pan_step(pan);
        self.ears.borrow_mut().engine_lean = step;
        if step != self.engine_pan_step {
            self.engine_pan_step = step;
            self.hear(format!("[engine] pans {}", pan_step_text(step)));
        }
        self.inner.set_engine_pan(pan);
    }
    fn set_loop_rate(&mut self, channel: u32, rate: f64) {
        self.inner.set_loop_rate(channel, rate);
    }
    fn stop_loop_with(&mut self, channel: u32, fade_ms: u32) {
        self.loop_stopped(channel);
        self.inner.stop_loop_with(channel, fade_ms);
    }
    fn start_sustain_loop_with(
        &mut self,
        channel: u32,
        key: &str,
        spec: SustainLoopSpec,
        volume: f64,
    ) {
        self.loop_started(channel, key);
        self.inner
            .start_sustain_loop_with(channel, key, spec, volume);
    }
    fn release_sustain_loop_with(&mut self, channel: u32, fade_ms: u32) {
        self.loop_stopped(channel);
        self.inner.release_sustain_loop_with(channel, fade_ms);
    }
    fn hold_alert_with(&mut self, key: &str, volume: f64, fade_ms: u32) {
        // Re-asserted every frame while it holds; an ear hears it start
        // once (a ramp-end stop flooded a listen with a hundred of these).
        let fresh = self.ears.borrow().held_alert.as_deref() != Some(key);
        if fresh {
            self.ears.borrow_mut().held_alert = Some(key.to_string());
            self.hear(format!("[alert] {key} holds"));
        }
        self.inner.hold_alert_with(key, volume, fade_ms);
    }
    fn release_alert_with(&mut self, fade_ms: u32) {
        let was_held = self.ears.borrow_mut().held_alert.take().is_some();
        if was_held {
            self.hear("[alert] released".to_string());
        }
        self.inner.release_alert_with(fade_ms);
    }
    fn hold_cue(&mut self, name: &str) {
        if !self.inner.cue_held(name) {
            self.hear(format!("[cue] {name} holds"));
        }
        self.inner.hold_cue(name);
    }
    fn cue_held(&self, name: &str) -> bool {
        self.inner.cue_held(name)
    }
    fn release_cue(&mut self, name: &str) {
        if self.inner.cue_held(name) {
            self.hear(format!("[cue] {name} released"));
        }
        self.inner.release_cue(name);
    }
    fn engine_start_with(&mut self, play_start_sound: bool) {
        // A silent start is the loop coming back after a menu or a resumed
        // trip: no crank plays, so the ear must not report one -- every
        // unpause read as an engine start (agent drive, 2026-09-01).
        if play_start_sound {
            self.hear("[engine] starting".to_string());
        } else {
            self.hear("[engine] running again, no crank".to_string());
        }
        self.inner.engine_start_with(play_start_sound);
    }
    fn engine_stop_with(&mut self, shutdown_sound: bool) {
        self.hear("[engine] shut down".to_string());
        self.inner.engine_stop_with(shutdown_sound);
    }
    fn update(&mut self, dt: f64) {
        self.inner.update(dt);
    }
    fn set_engine_rpm_with(&mut self, rpm: f64, throttle: f64) {
        self.ears.borrow_mut().engine_rpm.push(rpm);
        self.inner.set_engine_rpm_with(rpm, throttle);
    }
    fn set_road_noise(&mut self, speed_mps: f64) {
        self.ears.borrow_mut().road_noise_mps = Some(speed_mps);
        // The road bed starts and stops behind this call, so the tee's book
        // of what plays where has to move with it.
        if speed_mps < ROAD_BED_MIN_MPS {
            self.loop_stopped(CH_ROAD);
        } else {
            self.loop_started(CH_ROAD, ROAD_BED_KEY);
        }
        self.inner.set_road_noise(speed_mps);
    }
    fn set_weather_with(&mut self, key: Option<&str>, intensity: f64) {
        let audible_key = key.filter(|_| intensity > 0.0);
        if audible_key != self.weather_key.as_deref() {
            match audible_key {
                Some(key) => self.hear(format!("[weather] {key}")),
                None => self.hear("[weather] stopped".to_string()),
            }
            self.weather_key = audible_key.map(str::to_string);
        }
        self.inner.set_weather_with(key, intensity);
    }
    fn set_wind(&mut self, intensity: f64) {
        self.inner.set_wind(intensity);
    }
    fn set_ambient_with(&mut self, key: Option<&str>, volume: f64) {
        let audible_key = key.filter(|_| volume > 0.0);
        if audible_key != self.ambient_key.as_deref() {
            match audible_key {
                Some(key) => self.hear(format!("[ambience] {key}")),
                None => self.hear("[ambience] stopped".to_string()),
            }
            self.ambient_key = audible_key.map(str::to_string);
        }
        self.inner.set_ambient_with(key, volume);
    }
    fn horn_start(&mut self) {
        self.hear("[horn] on".to_string());
        self.inner.horn_start();
    }
    fn horn_stop(&mut self) {
        self.hear("[horn] off".to_string());
        self.inner.horn_stop();
    }
    fn reverse_start(&mut self) {
        self.hear("[reverse beeper] on".to_string());
        self.inner.reverse_start();
    }
    fn reverse_stop(&mut self) {
        self.hear("[reverse beeper] off".to_string());
        self.inner.reverse_stop();
    }
    fn stop_world(&mut self) {
        // The engine loop drops silently with the rest of the road (a pause,
        // an arrival) and comes back silently; say so, or the return reads
        // as a start out of nowhere.
        if self.inner.engine_running() {
            self.hear("[engine] quiet while the road is paused".to_string());
        }
        if self.weather_key.take().is_some() {
            self.hear("[weather] stopped".to_string());
        }
        if self.ambient_key.take().is_some() {
            self.hear("[ambience] stopped".to_string());
        }
        self.loop_keys.clear();
        self.loop_pan_steps.clear();
        self.inner.stop_world();
    }
    fn play_music_with(&mut self, track: &str, fade_ms: u32) {
        self.hear(format!("[radio] now playing {track}"));
        self.inner.play_music_with(track, fade_ms);
    }
    fn play_radio_stream_with(&mut self, url: &str, fade_ms: u32) -> Result<(), AudioError> {
        self.hear("[radio] live stream tuning".to_string());
        self.inner.play_radio_stream_with(url, fade_ms)
    }
    fn play_music_file_with(&mut self, path: &str, fade_ms: u32) -> Result<(), AudioError> {
        self.inner.play_music_file_with(path, fade_ms)
    }
    fn music_playing(&self) -> bool {
        self.inner.music_playing()
    }
    fn music_length_s(&self) -> Option<f64> {
        self.inner.music_length_s()
    }
    fn radio_now_playing(&self) -> Option<String> {
        self.inner.radio_now_playing()
    }
    fn stop_music_with(&mut self, fade_ms: u32) {
        self.inner.stop_music_with(fade_ms);
    }
    fn set_volumes(&mut self, volumes: &VolumeUpdate) {
        self.inner.set_volumes(volumes);
    }
    fn shutdown(&mut self) {
        self.inner.shutdown();
    }
}

/// Wrap the app's live speech and audio in recording tees.
pub fn install_ears(app: &mut App) -> SharedEars {
    use crate::audio::{AudioEngine, NullBackend};
    let ears = Ears::shared();
    let speech = std::mem::replace(&mut app.ctx.speech, Box::new(crate::speech::NullSpeech));
    app.ctx.speech = Box::new(TeeSpeech {
        inner: speech,
        ears: Rc::clone(&ears),
    });
    let audio = std::mem::replace(
        &mut app.ctx.audio,
        Box::new(AudioEngine::with_backend(Box::new(NullBackend::new()))),
    );
    app.ctx.audio = Box::new(TeeAudio {
        inner: audio,
        ears: Rc::clone(&ears),
        weather_key: None,
        ambient_key: None,
        loop_keys: HashMap::new(),
        loop_pan_steps: HashMap::new(),
        engine_pan_step: 0,
    });
    ears
}

pub(super) fn drain_ears(ears: &SharedEars) -> String {
    let mut e = ears.borrow_mut();
    let mut lines: Vec<String> = Vec::new();
    let pending = std::mem::take(&mut e.lines);
    let sound_count = pending
        .iter()
        .filter(|line| !line.starts_with("[spoken]") && !line.starts_with("[spoken:event]"))
        .count();
    let omitted = sound_count.saturating_sub(MAX_SOUND_LINES);
    if omitted > 0 {
        lines.push(format!(
            "[sounds] {omitted} sound updates omitted from this listen"
        ));
    }
    let mut sounds_to_skip = omitted;
    for line in pending {
        let spoken = line.starts_with("[spoken]") || line.starts_with("[spoken:event]");
        if !spoken && sounds_to_skip > 0 {
            sounds_to_skip -= 1;
            continue;
        }
        lines.push(line);
    }
    if let (Some(first), Some(last)) = (e.engine_rpm.first(), e.engine_rpm.last()) {
        let low = e.engine_rpm.iter().cloned().fold(f64::MAX, f64::min);
        let high = e.engine_rpm.iter().cloned().fold(0.0_f64, f64::max);
        if (high - low).abs() > 25.0 {
            lines.push(format!(
                "[engine] {first:.0} rpm -> {last:.0} rpm (ranged {low:.0} to {high:.0})"
            ));
        }
    }
    e.engine_rpm.clear();
    if let Some(mps) = e.road_noise_mps.take() {
        if mps > 1.0 {
            lines.push(format!(
                "[road] rolling at about {:.0} miles per hour by ear",
                mps * 2.236_936
            ));
            // Where the two leans stand at this moment, heard or not.
            let bed = e
                .road_bed_lean
                .map_or_else(|| "silent".to_string(), pan_step_text);
            lines.push(format!(
                "[now] engine lean {}, road bed {bed}",
                pan_step_text(e.engine_lean)
            ));
        }
    }
    if lines.is_empty() {
        "(quiet -- nothing new to hear)".to_string()
    } else {
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{AudioEngine, NullBackend};
    fn tee_audio(ears: &SharedEars) -> TeeAudio {
        TeeAudio {
            inner: Box::new(AudioEngine::with_backend(Box::new(NullBackend::new()))),
            ears: Rc::clone(ears),
            weather_key: None,
            ambient_key: None,
            loop_keys: HashMap::new(),
            loop_pan_steps: HashMap::new(),
            engine_pan_step: 0,
        }
    }

    #[test]
    fn the_steering_guide_reaches_an_agents_ears() {
        // With lane keeping off the engine leans toward where the wheel
        // should go and the road bed sits where the truck is in its lane. Those
        // two are the whole instrument, and an agent asked to test steering
        // heard NEITHER until 2026-09-18: one-shots reported their side, but
        // the continuous pans went straight through to the backend.
        //
        // The bed is started the way the game starts it -- `set_road_noise`,
        // never by key -- because that is the path on which the tee had no
        // key for the channel and reported the lean as "channel 3" (S1).
        let ears = Ears::shared();
        {
            let mut audio = tee_audio(&ears);
            // Parked, the lane-position pan is written before any bed plays:
            // nothing to hear, so nothing said.
            audio.set_loop_pan(CH_ROAD, 0.4);
            audio.set_road_noise(20.0);
            audio.set_road_noise(21.0); // still rolling: not a second start

            // The bed leans into a left-hander and comes back.
            audio.set_loop_pan(CH_ROAD, -0.5);
            audio.set_loop_pan(CH_ROAD, -0.52); // same step: says nothing
            audio.set_loop_pan(CH_ROAD, 0.0);
            // And the engine follows the truck's own lane position.
            audio.set_engine_pan(0.75);
            audio.set_engine_pan(0.74); // same step again
        }
        let heard = drain_ears(&ears);
        assert!(
            !heard.contains("channel"),
            "the road bed must report by its key, not its channel: {heard}"
        );
        assert_eq!(
            heard.matches("[sound bed] vehicle/road starts").count(),
            1,
            "{heard}"
        );
        assert!(
            heard.contains("[bed] vehicle/road pans left 2"),
            "the guide's lean never reached the ears: {heard}"
        );
        assert!(heard.contains("[bed] vehicle/road pans centred"), "{heard}");
        assert!(heard.contains("[engine] pans right 3"), "{heard}");
        // Held steady, a pan is silent -- otherwise it floods every frame.
        assert_eq!(heard.matches("[engine] pans").count(), 1, "{heard}");
        assert_eq!(
            heard.matches("[bed] vehicle/road pans").count(),
            2,
            "{heard}"
        );
    }

    #[test]
    fn a_repeating_cue_reaches_the_engine_held_not_as_a_fresh_play() {
        // A blinker click is play-if-idle: the engine starts it only when the
        // last one has finished, and holds it as a cue. Through the tee it
        // used to arrive as a plain play, held by nobody, and every click
        // stacked a second copy on the first.
        let ears = Ears::shared();
        let mut audio = tee_audio(&ears);
        audio.play_if_idle("vehicle/turn_signal", 0.8, -0.6);
        assert!(audio.inner.cue_held("vehicle/turn_signal"));
        audio.release_cue("vehicle/turn_signal");
        let heard = drain_ears(&ears);
        assert_eq!(
            heard.matches("[cue] vehicle/turn_signal holds").count(),
            1,
            "{heard}"
        );
    }

    #[test]
    fn every_listen_says_where_the_leans_stand_now() {
        // A lean held steady says nothing, so after a quiet stretch an agent
        // steering by ear did not know where it stood and steered off old
        // news (agent drive with lane keeping off, 2026-09-23). Each listen
        // ends with where both stand, whether they moved or not.
        let ears = Ears::shared();
        {
            let mut audio = tee_audio(&ears);
            audio.set_road_noise(25.0);
            audio.set_loop_pan(CH_ROAD, 0.26);
            audio.set_engine_pan(-0.5);
        }
        let first = drain_ears(&ears);
        assert!(
            first.contains("[now] engine lean left 2, road bed right 1"),
            "{first}"
        );
        {
            let mut audio = tee_audio(&ears);
            audio.set_road_noise(25.0);
        }
        let quiet = drain_ears(&ears);
        assert!(!quiet.contains("[engine] pans"), "{quiet}");
        assert!(
            quiet.contains("[now] engine lean left 2, road bed right 1"),
            "held steady, the lean is still reported: {quiet}"
        );
    }

    #[test]
    fn a_bed_that_stops_forgets_its_lean_and_a_pan_with_no_bed_is_not_heard() {
        // `loop_pan_steps` was never cleared with `loop_keys`, so a bed that
        // stopped leaning left and came back centred said nothing about it,
        // and a pan on a silent channel was reported as "channel N".
        let ears = Ears::shared();
        {
            let mut audio = tee_audio(&ears);
            audio.start_loop_with(4, "poi/weigh_station_lane", 0.5, 0);
            audio.set_loop_pan(4, -0.6);
            audio.stop_loop_with(4, 0);
            audio.set_loop_pan(4, -0.6); // nothing playing: not a sound
            audio.start_loop_with(4, "poi/weigh_station_lane", 0.5, 0);
            audio.set_loop_pan(4, -0.6); // a fresh bed, leaning: said again

            // The road bed, stopped by the truck coming to rest, the same.
            audio.set_road_noise(20.0);
            audio.set_loop_pan(CH_ROAD, 0.5);
            audio.set_road_noise(0.0);
            audio.set_loop_pan(CH_ROAD, 0.5);
            audio.set_road_noise(20.0);
            audio.set_loop_pan(CH_ROAD, 0.5);

            // And a paused world drops every bed's lean with the bed.
            audio.stop_world();
            audio.set_loop_pan(CH_ROAD, 0.5);
            audio.set_road_noise(20.0);
            audio.set_loop_pan(CH_ROAD, 0.5);
        }
        let heard = drain_ears(&ears);
        assert!(!heard.contains("channel"), "{heard}");
        assert_eq!(
            heard
                .matches("[bed] poi/weigh_station_lane pans left 2")
                .count(),
            2,
            "{heard}"
        );
        assert_eq!(
            heard.matches("[bed] vehicle/road pans right 2").count(),
            3,
            "{heard}"
        );
    }

    #[test]
    fn only_an_interrupting_cab_line_reads_as_cutting_in() {
        let ears = Ears::shared();
        let mut tee = TeeSpeech {
            inner: Box::new(crate::speech::capture::NullSpeech),
            ears: Rc::clone(&ears),
        };
        tee.say_event("Exit lane opening. Steer right into it.", true);
        tee.say_event("Billboard: truck parking.", false);
        tee.say("Speed limit 60 miles per hour.", true);
        let lines = ears.borrow().lines.clone();
        let cut: Vec<bool> = lines.iter().map(|l| l.starts_with(CAB_CUT_IN)).collect();
        assert_eq!(cut, vec![true, false, false], "{lines:#?}");
    }

    #[test]
    fn spoken_lines_survive_a_sound_flood_in_chronological_order() {
        let ears = Ears::shared();
        {
            let mut recorded = ears.borrow_mut();
            recorded.lines.push("[spoken] before the flood".to_string());
            for index in 0..MAX_SOUND_LINES + 10 {
                recorded.lines.push(format!("[sound] update {index}"));
            }
            recorded
                .lines
                .push("[spoken:event] after the flood".to_string());
        }

        let heard = drain_ears(&ears);
        let before = heard.find("[spoken] before the flood").unwrap();
        let after = heard.find("[spoken:event] after the flood").unwrap();

        assert!(before < after, "{heard}");
        assert!(heard.contains("[sounds] 10 sound updates omitted from this listen"));
    }

    #[test]
    fn ears_report_only_real_cue_and_alert_transitions() {
        let ears = Ears::shared();
        let mut audio = tee_audio(&ears);

        audio.release_cue("turn_signal");
        audio.release_cue("turn_signal");
        audio.hold_cue("turn_signal");
        audio.hold_cue("turn_signal");
        audio.release_cue("turn_signal");
        audio.release_cue("turn_signal");
        audio.release_alert();
        audio.hold_alert("alert/lane_departure");
        audio.hold_alert("alert/lane_departure");
        audio.release_alert();
        audio.release_alert();

        assert_eq!(
            drain_ears(&ears).lines().collect::<Vec<_>>(),
            vec![
                "[cue] turn_signal holds",
                "[cue] turn_signal released",
                "[alert] alert/lane_departure holds",
                "[alert] released",
            ]
        );
    }

    #[test]
    fn ears_tell_a_silent_engine_return_from_a_crank() {
        // Unpausing brings the engine loop back without the ignition
        // one-shot; the ear used to call both "[engine] starting".
        let ears: SharedEars = Rc::new(RefCell::new(Ears::default()));
        let mut audio = tee_audio(&ears);

        audio.engine_start();
        audio.stop_world();
        audio.engine_start_with(false);

        assert_eq!(
            drain_ears(&ears)
                .lines()
                .filter(|line| line.starts_with("[engine]"))
                .collect::<Vec<_>>(),
            vec!["[engine] starting", "[engine] running again, no crank"]
        );
    }

    #[test]
    fn ears_report_a_continuing_sound_bed_once_until_it_stops() {
        let ears: SharedEars = Rc::new(RefCell::new(Ears::default()));
        let mut audio = tee_audio(&ears);

        audio.start_loop_with(4, "poi/weigh_station_lane", 0.5, 0);
        audio.start_loop_with(4, "poi/weigh_station_lane", 0.6, 0);
        assert_eq!(
            drain_ears(&ears)
                .lines()
                .filter(|line| *line == "[sound bed] poi/weigh_station_lane starts")
                .count(),
            1
        );

        audio.stop_loop_with(4, 0);
        audio.start_loop_with(4, "poi/weigh_station_lane", 0.5, 0);
        assert_eq!(
            drain_ears(&ears),
            "[sound bed] poi/weigh_station_lane starts"
        );
    }

    #[test]
    fn ears_report_ambient_transitions_without_frame_by_frame_repeats() {
        let ears: SharedEars = Rc::new(RefCell::new(Ears::default()));
        let mut audio = tee_audio(&ears);

        audio.set_ambient_with(Some("ambience/night"), 0.4);
        audio.set_ambient_with(Some("ambience/night"), 0.5);
        audio.set_ambient_with(None, 0.0);
        audio.set_ambient_with(Some("ambience/night"), 0.4);

        assert_eq!(
            drain_ears(&ears).lines().collect::<Vec<_>>(),
            vec![
                "[ambience] ambience/night",
                "[ambience] stopped",
                "[ambience] ambience/night",
            ]
        );
    }

    #[test]
    fn stopping_world_allows_the_same_sound_bed_to_be_reported_again() {
        let ears: SharedEars = Rc::new(RefCell::new(Ears::default()));
        let mut audio = tee_audio(&ears);

        audio.start_loop_with(4, "poi/weigh_station_lane", 0.5, 0);
        audio.stop_world();
        audio.start_loop_with(4, "poi/weigh_station_lane", 0.5, 0);

        assert_eq!(
            drain_ears(&ears)
                .lines()
                .filter(|line| *line == "[sound bed] poi/weigh_station_lane starts")
                .count(),
            2
        );
    }

    #[test]
    fn ears_report_a_continuing_weather_bed_once() {
        let ears: SharedEars = Rc::new(RefCell::new(Ears::default()));
        let mut audio = tee_audio(&ears);

        audio.set_weather_with(Some("weather/rain_light"), 0.7);
        audio.set_weather_with(Some("weather/rain_light"), 0.7);

        assert_eq!(
            drain_ears(&ears)
                .lines()
                .filter(|line| *line == "[weather] weather/rain_light")
                .count(),
            1
        );
    }

    #[test]
    fn ears_report_weather_stopping_and_restarting() {
        let ears: SharedEars = Rc::new(RefCell::new(Ears::default()));
        let mut audio = tee_audio(&ears);

        audio.set_weather_with(Some("weather/rain_light"), 0.7);
        audio.set_weather_with(None, 0.0);
        audio.set_weather_with(Some("weather/rain_light"), 0.7);

        assert_eq!(
            drain_ears(&ears).lines().collect::<Vec<_>>(),
            vec![
                "[weather] weather/rain_light",
                "[weather] stopped",
                "[weather] weather/rain_light",
            ]
        );
    }

    #[test]
    fn stopping_world_allows_the_same_weather_to_be_reported_again() {
        let ears: SharedEars = Rc::new(RefCell::new(Ears::default()));
        let mut audio = tee_audio(&ears);

        audio.set_weather_with(Some("weather/rain_light"), 0.7);
        audio.stop_world();
        audio.set_weather_with(Some("weather/rain_light"), 0.7);

        assert_eq!(
            drain_ears(&ears)
                .lines()
                .filter(|line| *line == "[weather] weather/rain_light")
                .count(),
            2
        );
    }
}
