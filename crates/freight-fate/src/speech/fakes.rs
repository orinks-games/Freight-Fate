//! Backend-level test doubles: a scripted registry and recording voices.
//!
//! These are `tests/test_speech_audio.py`'s `FakeContext`, `FakeBackend` and
//! `RecordingBackend` with a Rust shape, so the selection policy and the
//! [`super::Speech`] object can be driven without Prism. They are compiled
//! into the library (not `cfg(test)`) because the integration tests and the
//! app-shell tests need them too.

use std::cell::RefCell;
use std::rc::Rc;

use super::backend::{BackendId, VoiceBackend, VoiceFeatures, VoiceRegistry};

/// Everything a [`FakeVoice`] remembers. Shared between the handle the test
/// holds and every handle the registry hands out, because Prism, too, hands
/// back the same cached instance on every `acquire`.
#[derive(Debug, Default)]
pub struct FakeVoiceState {
    pub name: String,
    pub priority: i32,
    pub features: VoiceFeatures,
    pub voices: Vec<String>,
    pub rate: Option<f64>,
    pub pitch: Option<f64>,
    pub volume: Option<f64>,
    pub voice: Option<usize>,
    /// `(text, interrupt)` in the order spoken, `output` and `speak` alike.
    pub spoken: Vec<(String, bool)>,
    /// Text sent to the braille display, in order.
    pub brailled: Vec<String>,
    pub stop_calls: u32,
    /// When set, every `output`/`speak` fails the way a quit screen reader
    /// fails.
    pub fail_output: bool,
    /// When set, `braille` fails while speech still works: a screen reader
    /// whose display just unplugged.
    pub fail_braille: bool,
}

/// A recording voice: `RecordingBackend` in the Python tests. Cloning the
/// handle shares the state.
#[derive(Clone, Debug)]
pub struct FakeVoice {
    state: Rc<RefCell<FakeVoiceState>>,
}

impl FakeVoice {
    pub fn new(name: &str, priority: i32, features: VoiceFeatures) -> Self {
        Self {
            state: Rc::new(RefCell::new(FakeVoiceState {
                name: name.to_string(),
                priority,
                features,
                ..FakeVoiceState::default()
            })),
        }
    }

    /// A voice with installed voices to pick from.
    pub fn with_voices(
        name: &str,
        priority: i32,
        features: VoiceFeatures,
        voices: &[&str],
    ) -> Self {
        let voice = Self::new(name, priority, features);
        voice.state.borrow_mut().voices = voices.iter().map(|v| v.to_string()).collect();
        voice
    }

    /// Borrow the recorded state (spoken lines, parameters, stop count).
    pub fn state(&self) -> std::cell::Ref<'_, FakeVoiceState> {
        self.state.borrow()
    }

    /// Mutate the state between calls: flip `is_supported_at_runtime`, set
    /// `fail_output`, and so on.
    pub fn state_mut(&self) -> std::cell::RefMut<'_, FakeVoiceState> {
        self.state.borrow_mut()
    }

    pub fn set_runtime_supported(&self, supported: bool) {
        self.state.borrow_mut().features.is_supported_at_runtime = supported;
    }

    pub fn set_fail_output(&self, fail: bool) {
        self.state.borrow_mut().fail_output = fail;
    }

    pub fn set_fail_braille(&self, fail: bool) {
        self.state.borrow_mut().fail_braille = fail;
    }

    /// `(text, interrupt)` pairs spoken so far.
    pub fn spoken(&self) -> Vec<(String, bool)> {
        self.state.borrow().spoken.clone()
    }

    /// Lines sent to the braille display so far.
    pub fn brailled(&self) -> Vec<String> {
        self.state.borrow().brailled.clone()
    }

    pub fn stop_calls(&self) -> u32 {
        self.state.borrow().stop_calls
    }

    pub fn priority(&self) -> i32 {
        self.state.borrow().priority
    }

    /// A boxed handle sharing this voice's state.
    pub fn boxed(&self) -> Box<dyn VoiceBackend> {
        Box::new(self.clone())
    }

    fn failure() -> prismer::Error {
        prismer::Error::SpeakFailure
    }
}

impl VoiceBackend for FakeVoice {
    fn name(&self) -> String {
        self.state.borrow().name.clone()
    }

    fn features(&self) -> VoiceFeatures {
        self.state.borrow().features
    }

    fn output(&mut self, text: &str, interrupt: bool) -> Result<(), prismer::Error> {
        // Prism's output() for a braille-capable screen reader: speak, then
        // braille, and the call fails if the braille half does, after the
        // line was already spoken.
        self.speak(text, interrupt)?;
        if self.state.borrow().features.supports_braille {
            self.braille(text)?;
        }
        Ok(())
    }

    fn speak(&mut self, text: &str, interrupt: bool) -> Result<(), prismer::Error> {
        let mut state = self.state.borrow_mut();
        if state.fail_output {
            return Err(Self::failure());
        }
        state.spoken.push((text.to_string(), interrupt));
        Ok(())
    }

    fn braille(&mut self, text: &str) -> Result<(), prismer::Error> {
        let mut state = self.state.borrow_mut();
        if !state.features.supports_braille || state.fail_braille {
            return Err(Self::failure());
        }
        state.brailled.push(text.to_string());
        Ok(())
    }

    fn stop(&mut self) -> Result<(), prismer::Error> {
        self.state.borrow_mut().stop_calls += 1;
        Ok(())
    }

    fn set_rate(&mut self, rate: f64) -> Result<(), prismer::Error> {
        self.state.borrow_mut().rate = Some(rate);
        Ok(())
    }

    fn set_pitch(&mut self, pitch: f64) -> Result<(), prismer::Error> {
        self.state.borrow_mut().pitch = Some(pitch);
        Ok(())
    }

    fn set_volume(&mut self, volume: f64) -> Result<(), prismer::Error> {
        self.state.borrow_mut().volume = Some(volume);
        Ok(())
    }

    fn voices_count(&self) -> Result<usize, prismer::Error> {
        Ok(self.state.borrow().voices.len())
    }

    fn voice_name(&self, index: usize) -> Result<String, prismer::Error> {
        self.state
            .borrow()
            .voices
            .get(index)
            .cloned()
            .ok_or_else(Self::failure)
    }

    fn set_voice(&mut self, index: usize) -> Result<(), prismer::Error> {
        self.state.borrow_mut().voice = Some(index);
        Ok(())
    }
}

/// Mimics `prism.Context`: a static, priority-ordered backend registry
/// (`FakeContext` in the Python tests). Ids are the 1-based position in the
/// priority-sorted order, so a test can also reason about them.
#[derive(Clone, Debug, Default)]
pub struct FakeRegistry {
    order: Vec<FakeVoice>,
}

impl FakeRegistry {
    /// Register `voices`, ranked by priority (highest first; ties keep the
    /// given order, as Python's stable sort did).
    pub fn new(voices: Vec<FakeVoice>) -> Self {
        let mut order = voices;
        order.sort_by_key(|voice| std::cmp::Reverse(voice.priority()));
        Self { order }
    }

    /// The registered voice named `name`, to inspect or mutate it.
    pub fn voice(&self, name: &str) -> Option<&FakeVoice> {
        self.order.iter().find(|voice| voice.name() == name)
    }

    fn index_of(&self, id: BackendId) -> Option<usize> {
        let index = usize::try_from(id).ok()?.checked_sub(1)?;
        (index < self.order.len()).then_some(index)
    }
}

impl VoiceRegistry for FakeRegistry {
    fn backend_count(&self) -> usize {
        self.order.len()
    }

    fn id_at(&self, index: usize) -> Option<BackendId> {
        (index < self.order.len()).then(|| index as BackendId + 1)
    }

    fn id_by_name(&self, name: &str) -> Option<BackendId> {
        self.order
            .iter()
            .position(|voice| voice.name() == name)
            .map(|index| index as BackendId + 1)
    }

    fn name_of(&self, id: BackendId) -> Option<String> {
        self.index_of(id).map(|index| self.order[index].name())
    }

    fn priority_of(&self, id: BackendId) -> i32 {
        self.index_of(id)
            .map(|index| self.order[index].priority())
            .unwrap_or(0)
    }

    fn acquire(&self, id: BackendId) -> Result<Box<dyn VoiceBackend>, prismer::Error> {
        self.index_of(id)
            .map(|index| self.order[index].boxed())
            .ok_or(prismer::Error::BackendNotAvailable)
    }
}
