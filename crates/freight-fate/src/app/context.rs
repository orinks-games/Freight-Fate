//! [`GameContext`]: the shared services handed to every state (the
//! `GameContext` class of `freight_fate/app.py`), plus the state stack it
//! owns in this port.
//!
//! # Why the stack lives here
//!
//! In Python a state called `self.ctx.push_state(...)` and the new screen's
//! `enter()` spoke *immediately*, in the middle of the caller's handler; the
//! transcript tests pin that order. Rust cannot hand a state `&mut App`
//! while `App` is calling into it, so the stack is `Vec<Rc<RefCell<dyn
//! State>>>` owned by the context: a lifecycle call on any state that is not
//! currently borrowed runs at once, exactly as in Python, and the one case
//! that cannot -- `exit()` on the very state whose handler asked to be
//! popped (it is borrowed mutably for the duration of its own handler) -- is
//! queued and run the moment that handler returns ([`GameContext::run_deferred`],
//! which the app calls after every dispatch). Only that `exit` moves; every
//! `enter` and every other `exit` keeps its Python position.
//!
//! The speech delivery half (`say`, `say_event`, the ladder, the pacer, the
//! duck) is in `app::speech_delivery`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use ff_core::achievements::{award, AchievementAward};
use ff_core::data::world::World;
use ff_core::l10n::Translator;
use ff_core::message_log::{MessageCategory, MessageLog};
use ff_core::models::economy::Economy;
use ff_core::models::profile::Profile;
use ff_core::music_synth::SynthWorker;
use ff_core::playtest_levers::LeverContext;
use ff_core::settings::Settings;
use ff_core::sim::real_fuel_price::FuelPriceProvider;
use ff_core::sim::real_traffic::RealTrafficProvider;
use ff_core::sim::real_weather::RealWeatherProvider;
use ff_core::sim::real_weather_alerts::WeatherAlertsProvider;
use ff_core::sim::truck_parking::TruckParkingProvider;
use ff_core::speech_text::achievement_announced;

use crate::account_achievements::AccountAchievements;
use crate::audio::{Audio, VolumeUpdate};
use crate::bindings::KeyBindings;
use crate::cloud_saves::{BackupAnnouncements, CloudSaves};
use crate::controller::ControllerManager;
use crate::discord_presence::DiscordPresence;
use crate::duty_watch::DutyWatch;
use crate::jaws_script::JawsScript;
use crate::meaningful_play::MeaningfulPlayReason;
use crate::net::UreqTransport;
use crate::online_journal::{queue_achievement, JournalOutbox};
use crate::online_presence::{OnlineIdentity, OnlinePresence};
use crate::speech::{apply_speech_settings, EventSpeechPacer, SpeechSink};
use crate::states::base::State;

use super::held_keys::HeldKeys;
use super::Say;

/// A state on the stack.
pub type SharedState = Rc<RefCell<dyn State>>;

/// Wrap a state for the stack.
pub fn share<S: State + 'static>(state: S) -> SharedState {
    Rc::new(RefCell::new(state))
}

/// The system clipboard (SDL's in the windowed app, a string in headless
/// runs). `pygame.scrap` / `write_clipboard_text`.
pub trait Clipboard {
    fn get_text(&self) -> Option<String>;
    /// True when the text landed on the clipboard.
    fn set_text(&mut self, text: &str) -> bool;
}

/// An in-memory clipboard for headless runs and tests.
#[derive(Debug, Default)]
pub struct MemoryClipboard {
    pub text: Option<String>,
}

impl Clipboard for MemoryClipboard {
    fn get_text(&self) -> Option<String> {
        self.text.clone()
    }

    fn set_text(&mut self, text: &str) -> bool {
        self.text = Some(text.to_string());
        true
    }
}

struct StackEntry {
    state: SharedState,
    /// `paces_main_speech`, read when the state was pushed: the stack's
    /// top may be borrowed for its own handler when `say` needs the answer.
    paces_main_speech: bool,
}

enum Deferred {
    Enter(SharedState),
    Exit(SharedState),
}

/// The online and sharing services the app runs beside the game.
pub struct Services {
    pub presence: DiscordPresence,
    pub online: OnlinePresence,
    /// The background watch on the drivers list (the "Say when drivers go
    /// on or off duty" row).
    pub duty: DutyWatch,
    /// The optional JAWS arrow-key script (Settings, Speech).
    pub jaws_script: JawsScript,
    pub cloud: CloudSaves,
    pub journal: JournalOutbox,
}

/// A career achievement and whether this installation can publish it publicly.
///
/// The award itself remains career-specific. `publicly_eligible` is true only
/// after the account ledger has durably recorded the achievement for the first
/// time, so callers building public event reasons cannot mistake a career-new
/// award for an account-new one.
pub struct AwardedAchievement {
    pub award: AchievementAward,
    pub publicly_eligible: bool,
}

/// Shared services handed to every state.
pub struct GameContext {
    pub speech: Box<dyn SpeechSink>,
    pub audio: Box<dyn Audio>,
    pub controller: ControllerManager,
    pub settings: Settings,
    /// Which key and pad button does what at the wheel, rebuilt from
    /// `settings` by `apply_bindings` whenever the player moves one.
    pub bindings: KeyBindings,
    pub world: &'static World,
    pub economy: Economy,
    pub profile: Option<Profile>,
    /// Achievements known anywhere on this installation, distinct from the
    /// active career's progress and reward state.
    pub account_achievements: AccountAchievements,
    pub message_log: MessageLog,
    /// Turns each finished English line into the player's language
    /// (`app::language`); English passes lines through untouched.
    pub translator: Arc<Translator>,
    pub achievement_notice: String,
    pub achievement_notice_timer: f64,
    /// True while a playtest-lever scenario runs unsaved (see
    /// `playtest_levers::apply_continue_levers`); `save_profile` honors it.
    pub playtest_sandbox: bool,
    /// Seed for the next dispatch board; `None` in the shipped game, where
    /// the board rolls from OS entropy. The test app sets it so that which
    /// load dispatch assigns -- and whether it is staged in the home yard,
    /// which opens the shipping office instead of a deadhead -- is the same
    /// on every run instead of a coin toss (the smoke test failed two runs
    /// in three once yard loads existed, 2026-09-02). Each board built
    /// advances it by one, so a career that comes back to the board is not
    /// offered the identical set again.
    pub dispatch_board_seed: Option<i64>,
    /// Driving-school sandbox: the profile is a throwaway copy and must
    /// never reach disk; the real save is restored when school ends.
    pub school_sandbox: bool,
    pub school_real_profile: Option<Profile>,
    /// Anti-backlog projection for the dedicated event voice: queued driving
    /// events that would start speaking stale get flushed instead.
    pub event_pacer: EventSpeechPacer,
    pub clipboard: Box<dyn Clipboard>,
    pub services: Services,
    pub input: HeldKeys,
    /// `App.running`.
    pub running: bool,

    // -- speech delivery state (app::speech_delivery) -------------------------------
    /// Whether the game mix is currently stepped down under the event voice
    /// (Settings > Audio; see `engage_speech_duck`).
    pub(crate) speech_ducked: bool,
    /// True only while a control the player actually pressed is being
    /// handled. See `player_asked`: a readout somebody asked for cuts the
    /// line in progress even at the wheel, where unasked-for lines queue.
    pub(crate) speech_requested: bool,
    /// The co-driver has the wheel and the player is in the sleeper: the
    /// road, the co-driver's own controls and the screens they work say
    /// nothing aloud (the transcript still gets every line). Answers to
    /// keys the player presses, and forced lines such as the co-driver's
    /// summon, still speak.
    pub co_driver_away: bool,
    /// The co-driver's shift is being slept through: the app runs the
    /// drive ahead several frames at a time, with the sound down.
    pub co_driver_fast_forward: bool,
    /// Which screen and row had focus when the copy mark was last set.
    copy_focus_key: Option<(usize, usize)>,
    /// `message_log.total_added()` when the screen opened or focus last
    /// moved. While nothing newer is logged, the copy key copies the row.
    pub(crate) copy_mark: u64,
    /// `message_log.total_added()` at the end of the last input, so a
    /// screen's opening lines (or a page line) logged by the input that
    /// moved focus stay ahead of the mark and can be copied first.
    copy_seen: u64,
    /// FIRST_OCCURRENCE and TRANSITIONS need memory of what has already
    /// been said; Settings cannot hold it, so it lives here beside the
    /// pacer. `ladder_said` is leg-scoped ("once per leg"), `ladder_last`
    /// is the last text seen per key so a re-assertion can be told from a
    /// change of state.
    pub(crate) ladder_said: HashSet<(String, String)>,
    pub(crate) ladder_last: HashMap<String, String>,
    /// The cut lines the pacer has handed back, newest last (see
    /// `requeue_cut_event`). A hand-back reaches the voice as a second
    /// delivery of the same text, so anything counting what the GAME announced
    /// from what the VOICE received has to be able to tell the two apart; the
    /// playtest rig reads this. Bounded: only the recent ones are kept.
    pub(crate) handed_back: VecDeque<String>,

    // -- music rotation ----------------------------------------------------------------
    pub(crate) music_pool_positions: HashMap<(String, Vec<String>), usize>,
    pub(crate) music_pool_last: HashMap<String, String>,
    /// The CALLER's pool and sequence, so leaving Synthesized mode finds the
    /// soundtrack again (see `app::synth_music`).
    pub(crate) music_rotation_pool: Option<(String, Vec<String>)>,
    /// Whether the rotation was started in Synthesized mode; a flip since
    /// then restarts it instead of refreshing it.
    music_rotation_synth: bool,
    music_rotation_track: Option<String>,
    music_rotation_elapsed_s: f64,
    music_rotation_duration_s: f64,
    /// Renders synthesized pieces off the loop; shut down in `App::shutdown`.
    pub synth_worker: SynthWorker,

    // -- live-data providers, lazy and session-long -----------------------------------
    real_weather: Option<Arc<RealWeatherProvider>>,
    real_traffic: Option<Arc<RealTrafficProvider>>,
    weather_alerts: Option<Arc<WeatherAlertsProvider>>,
    fuel_prices: Option<Arc<FuelPriceProvider>>,
    truck_parking: Option<Arc<TruckParkingProvider>>,

    // -- the state stack ----------------------------------------------------------------
    stack: Vec<StackEntry>,
    deferred: Vec<Deferred>,
}

/// Everything [`GameContext::new`] takes.
pub struct ContextParts {
    pub speech: Box<dyn SpeechSink>,
    pub audio: Box<dyn Audio>,
    pub controller: ControllerManager,
    pub settings: Settings,
    pub world: &'static World,
    pub economy: Economy,
    pub account_achievements: AccountAchievements,
    pub message_log: MessageLog,
    pub clipboard: Box<dyn Clipboard>,
    pub services: Services,
}

impl GameContext {
    pub fn new(parts: ContextParts) -> Self {
        // The guide tone must exist before a drive starts, not only after
        // the Learn game sounds screen has been opened. Idempotent and cheap.
        ff_core::lane_guide_tone::register_lane_guide_tone();
        ff_core::cab_sounds::register_cab_sounds();
        Self {
            speech: parts.speech,
            audio: parts.audio,
            controller: parts.controller,
            bindings: KeyBindings::from_settings(&parts.settings),
            settings: parts.settings,
            world: parts.world,
            economy: parts.economy,
            profile: None,
            account_achievements: parts.account_achievements,
            message_log: parts.message_log,
            translator: Arc::new(Translator::english()),
            achievement_notice: String::new(),
            achievement_notice_timer: 0.0,
            playtest_sandbox: false,
            dispatch_board_seed: None,
            school_sandbox: false,
            school_real_profile: None,
            event_pacer: EventSpeechPacer::new(super::FF_PACER),
            clipboard: parts.clipboard,
            services: parts.services,
            input: HeldKeys::default(),
            running: false,
            speech_ducked: false,
            speech_requested: false,
            co_driver_away: false,
            co_driver_fast_forward: false,
            copy_focus_key: None,
            copy_mark: 0,
            copy_seen: 0,
            ladder_said: HashSet::new(),
            ladder_last: HashMap::new(),
            handed_back: VecDeque::new(),
            music_pool_positions: HashMap::new(),
            music_pool_last: HashMap::new(),
            music_rotation_pool: None,
            music_rotation_synth: false,
            music_rotation_track: None,
            music_rotation_elapsed_s: 0.0,
            music_rotation_duration_s: 0.0,
            synth_worker: SynthWorker::start(),
            real_weather: None,
            real_traffic: None,
            weather_alerts: None,
            fuel_prices: None,
            truck_parking: None,
            stack: Vec::new(),
            deferred: Vec::new(),
        }
    }

    /// True when both the master `online_services` switch and the
    /// individual `setting` are enabled.
    ///
    /// The master governs the orinks.net and sharing services only (drivers
    /// board, profile sharing, cloud backup, Discord presence).
    /// Live-data simulation sources -- weather, traffic, parking -- follow
    /// their own Settings toggles and are deliberately NOT gated here (owner
    /// ruling, 2026-08-08: two testers lost real weather to the master
    /// switch without a word of explanation).
    pub fn online_enabled(&self, setting: bool) -> bool {
        self.settings.online_services && setting
    }

    // -- live-data providers ----------------------------------------------------------

    /// Shared NWS provider when real weather is enabled, else None.
    ///
    /// Created lazily and kept for the whole session so its cache spans trips.
    pub fn real_weather_provider(&mut self) -> Option<&RealWeatherProvider> {
        if !self.settings.real_weather {
            return None;
        }
        if self.real_weather.is_none() {
            self.real_weather = Some(Arc::new(RealWeatherProvider::with_nws(Arc::new(
                UreqTransport,
            ))));
        }
        self.real_weather.as_deref()
    }

    /// Install the session's live-weather provider directly.
    ///
    /// The lazy constructor above always builds the shipped NWS provider over
    /// a real HTTP transport, which a test can neither reach nor let run. This
    /// is the seam Python had by assigning `ctx.real_weather_provider`: a
    /// provider built on a stub fetch, so the city screens can be asked what
    /// they say about a route whose cells are part answered, part failed and
    /// part still loading.
    pub fn set_real_weather_provider(&mut self, provider: Arc<RealWeatherProvider>) {
        self.real_weather = Some(provider);
    }

    /// Start fetching a city's live weather before any drive needs it.
    ///
    /// The provider caches observations per station, so a trip leaving this
    /// city finds its first route cell already answered and starts live
    /// instead of holding "loading". Quietly a no-op when real weather is
    /// off or the city is unknown.
    pub fn warm_real_weather(&mut self, city_key: &str) {
        let key = self.world.resolve_city_key(city_key);
        let Some(city) = self.world.cities.get(&key) else {
            return;
        };
        let (lat, lon) = (city.lat, city.lon);
        let Some(provider) = self.real_weather_provider() else {
            return;
        };
        if lat != 0.0 || lon != 0.0 {
            provider.request(&format!("city:{key}"), lat, lon);
        }
    }

    /// Shared state 511 provider when real traffic is enabled, else None.
    pub fn real_traffic_provider(&mut self) -> Option<&RealTrafficProvider> {
        if !self.settings.real_traffic {
            return None;
        }
        if self.real_traffic.is_none() {
            self.real_traffic = Some(Arc::new(RealTrafficProvider::new(Arc::new(UreqTransport))));
        }
        self.real_traffic.as_deref()
    }

    /// The same 511 provider as an owned handle, for a caller that has to
    /// keep changing the context while it consults the feeds. None when real
    /// traffic is off.
    pub fn real_traffic_provider_arc(&mut self) -> Option<Arc<RealTrafficProvider>> {
        self.real_traffic_provider()?;
        self.real_traffic.clone()
    }

    /// Put a provider in place of the live one. Tests seed an offline
    /// provider's cache with the construction they want dispatch to see.
    pub fn set_real_traffic_provider(&mut self, provider: Arc<RealTrafficProvider>) {
        self.real_traffic = Some(provider);
    }

    /// The National Weather Service alerts provider, shared for the session,
    /// when real weather is on. Dispatch at the pickup and the cab on the
    /// road read the same cache. None with real weather off.
    pub fn weather_alerts_provider_arc(&mut self) -> Option<Arc<WeatherAlertsProvider>> {
        if !self.settings.real_weather {
            return None;
        }
        if self.weather_alerts.is_none() {
            self.weather_alerts = Some(Arc::new(WeatherAlertsProvider::new(Arc::new(
                UreqTransport,
            ))));
        }
        self.weather_alerts.clone()
    }

    /// Put a provider in place of the live one. Tests seed an offline
    /// provider with the warnings they want dispatch and the cab to see.
    pub fn set_weather_alerts_provider(&mut self, provider: Arc<WeatherAlertsProvider>) {
        self.weather_alerts = Some(provider);
    }

    /// Bring the economy's diesel price in line with this week's federal
    /// survey figure when real fuel prices are on: ask the keyless feed
    /// (never blocking) and apply whatever has arrived. Off, the price drawn
    /// for the session stands. Cheap enough to run every frame.
    pub fn sync_fuel_prices(&mut self) {
        if !self.settings.real_fuel_prices {
            self.economy.set_live_national_price(None);
            return;
        }
        let provider = self
            .fuel_prices
            .get_or_insert_with(|| Arc::new(FuelPriceProvider::new(Arc::new(UreqTransport))))
            .clone();
        provider.request();
        self.economy
            .set_live_national_price(provider.get().map(|price| price.per_gal));
    }

    /// Put a provider in place of the live one. Tests seed an offline
    /// provider with the week's price they want the pumps to charge.
    pub fn set_fuel_price_provider(&mut self, provider: Arc<FuelPriceProvider>) {
        self.fuel_prices = Some(provider);
    }

    /// Shared TPIMS provider when real parking is enabled, else None.
    pub fn truck_parking_provider(&mut self) -> Option<&TruckParkingProvider> {
        if !self.settings.real_parking {
            return None;
        }
        if self.truck_parking.is_none() {
            self.truck_parking = Some(Arc::new(TruckParkingProvider::new(Arc::new(UreqTransport))));
        }
        self.truck_parking.as_deref()
    }

    // -- state stack ----------------------------------------------------------------------

    /// The active state, or None when the stack is empty.
    pub fn state(&self) -> Option<SharedState> {
        self.stack.last().map(|entry| Rc::clone(&entry.state))
    }

    /// Every state on the stack, bottom first.
    pub fn states(&self) -> Vec<SharedState> {
        self.stack
            .iter()
            .map(|entry| Rc::clone(&entry.state))
            .collect()
    }

    pub fn stack_len(&self) -> usize {
        self.stack.len()
    }

    /// Whether the active state paces main-channel speech (the Python
    /// `getattr(self._app.state, "paces_main_speech", False)`).
    pub fn top_paces_main_speech(&self) -> bool {
        self.stack
            .last()
            .is_some_and(|entry| entry.paces_main_speech)
    }

    fn enter_now_or_later(&mut self, state: &SharedState) {
        match state.try_borrow_mut() {
            Ok(mut s) => s.enter(self),
            Err(_) => self.deferred.push(Deferred::Enter(Rc::clone(state))),
        }
    }

    fn exit_now_or_later(&mut self, state: &SharedState) {
        match state.try_borrow_mut() {
            Ok(mut s) => s.exit(self),
            Err(_) => self.deferred.push(Deferred::Exit(Rc::clone(state))),
        }
    }

    /// Run every lifecycle call that had to wait for a borrowed state (the
    /// state that asked to be popped during its own handler). The app calls
    /// this after every dispatch and update; harmless when nothing waits.
    pub fn run_deferred(&mut self) {
        while !self.deferred.is_empty() {
            // take, not drain-collect: the whole queue moves out in one
            // swap instead of being copied element by element, and the
            // handlers below may push new deferred calls onto the empty
            // vec left behind, which the loop then picks up.
            let pending: Vec<Deferred> = std::mem::take(&mut self.deferred);
            for call in pending {
                match call {
                    Deferred::Enter(state) => match state.try_borrow_mut() {
                        Ok(mut s) => s.enter(self),
                        Err(_) => {
                            log::error!("a state stayed borrowed past its dispatch; enter dropped")
                        }
                    },
                    Deferred::Exit(state) => match state.try_borrow_mut() {
                        Ok(mut s) => s.exit(self),
                        Err(_) => {
                            log::error!("a state stayed borrowed past its dispatch; exit dropped")
                        }
                    },
                }
            }
        }
        self.note_copy_focus();
    }

    /// Set the copy mark when the top screen or its focused row changed. The
    /// mark is the log count from before the input that changed focus, so
    /// what that input logged (a screen's opening lines, a search result, a
    /// page line) is copied first; what came earlier (a re-read row) is
    /// behind the mark, so the next focus move copies the focused row.
    pub(crate) fn note_copy_focus(&mut self) {
        let Some(state) = self.state() else {
            self.copy_focus_key = None;
            return;
        };
        let Ok(borrowed) = state.try_borrow() else {
            return;
        };
        let key = borrowed
            .copy_focus(self)
            .map(|focus| (Rc::as_ptr(&state) as *const () as usize, focus.index));
        drop(borrowed);
        if key != self.copy_focus_key {
            self.copy_focus_key = key;
            self.copy_mark = self.copy_seen;
        }
        self.copy_seen = self.message_log.total_added();
    }

    /// Push `state` and enter it (`push_state(state, should_enter=True)`).
    pub fn push_state<S: State + 'static>(&mut self, state: S) {
        self.push_shared_with(share(state), true);
    }

    pub fn push_state_with<S: State + 'static>(&mut self, state: S, should_enter: bool) {
        self.push_shared_with(share(state), should_enter);
    }

    /// Push an already-shared state (a factory's product, the harness).
    pub fn push_shared(&mut self, state: SharedState) {
        self.push_shared_with(state, true);
    }

    pub fn push_shared_with(&mut self, shared: SharedState, should_enter: bool) {
        let paces_main_speech = shared
            .try_borrow()
            .map(|s| s.paces_main_speech())
            .unwrap_or(false);
        // A new screen never inherits the last one's held keys -- above all
        // a pulse a screen reader's re-injected press train left running.
        self.input.clear_pulses();
        self.stack.push(StackEntry {
            state: Rc::clone(&shared),
            paces_main_speech,
        });
        if should_enter {
            self.enter_now_or_later(&shared);
        }
    }

    /// Lift the top state off the stack, without deciding what follows.
    ///
    /// Emptying the stack only ends the game when the player backed out of
    /// the last state; the rebuilding methods below empty it on their way to
    /// a new state, so they use this instead of `pop_state`.
    fn take_top(&mut self, should_exit: bool) {
        self.input.clear_pulses();
        if let Some(entry) = self.stack.pop() {
            if should_exit {
                self.exit_now_or_later(&entry.state);
            }
        }
    }

    /// `pop_state()` with the defaults: exit the top, re-enter the revealed.
    pub fn pop_state(&mut self) {
        self.pop_state_with(true, true);
    }

    pub fn pop_state_with(&mut self, should_exit: bool, reentry: bool) {
        self.take_top(should_exit);
        match self.state() {
            Some(revealed) => {
                if reentry {
                    self.enter_now_or_later(&revealed);
                }
            }
            None => self.running = false,
        }
    }

    pub fn replace_state<S: State + 'static>(&mut self, state: S) {
        self.replace_shared_with(share(state), true, true);
    }

    pub fn replace_state_with<S: State + 'static>(
        &mut self,
        state: S,
        should_exit: bool,
        reentry: bool,
    ) {
        self.replace_shared_with(share(state), should_exit, reentry);
    }

    pub fn replace_shared_with(&mut self, state: SharedState, should_exit: bool, reentry: bool) {
        self.take_top(should_exit);
        self.push_shared_with(state, reentry);
    }

    pub fn reset_to<S: State + 'static>(&mut self, state: S) {
        self.reset_shared_with(share(state), true, true);
    }

    pub fn reset_to_with<S: State + 'static>(
        &mut self,
        state: S,
        should_exit: bool,
        reentry: bool,
    ) {
        self.reset_shared_with(share(state), should_exit, reentry);
    }

    pub fn reset_shared_with(&mut self, state: SharedState, should_exit: bool, reentry: bool) {
        while !self.stack.is_empty() {
            self.take_top(should_exit);
        }
        self.push_shared_with(state, reentry);
    }

    pub fn quit(&mut self) {
        self.running = false;
    }

    /// Apply a radio settings change to a covered drive, right now.
    ///
    /// Flipping streamer-safe from the pause settings is a promise about
    /// what is on the air at this moment; the active drive must react while
    /// the menu still covers it, not at some later reception tick.
    pub fn apply_active_radio_settings(&mut self) {
        let target = self
            .stack
            .iter()
            .rev()
            .map(|entry| Rc::clone(&entry.state))
            .find(|state| state.try_borrow().is_ok_and(|s| s.applies_radio_settings()));
        if let Some(state) = target {
            if let Ok(mut s) = state.try_borrow_mut() {
                s.apply_radio_settings_now(self);
            }
        }
    }

    /// Whether a drive under the menus has its radio on (the main menu's
    /// `_driving_radio_active`).
    pub fn driving_radio_active(&self) -> bool {
        for entry in self.stack.iter().rev() {
            if let Ok(state) = entry.state.try_borrow() {
                if let Some(radio) = state.radio() {
                    return radio.enabled;
                }
            }
        }
        false
    }

    // -- profile and settings reflectors ------------------------------------------------

    pub fn save_profile(&mut self) {
        // Driving-school sandbox: the profile is a throwaway copy and must
        // never reach disk; the real save is restored when school ends.
        // Playtest-lever sandbox: a forced scenario run is temporary by
        // default -- the career file on disk stays exactly as it was.
        if self.school_sandbox || self.playtest_sandbox {
            return;
        }
        if let Some(profile) = &self.profile {
            let save_name = profile.name.clone();
            let slot_name = crate::cloud_saves::save_slot_name(&save_name);
            let already_marked = self
                .services
                .cloud
                .meaningful_play_tracker()
                .for_upload(&slot_name)
                .is_some();
            let changed = if already_marked || !profile.path().exists() {
                false
            } else {
                Profile::load(&profile.path())
                    .ok()
                    .map(|saved| {
                        let saved = serde_json::Value::Object(saved.to_dict());
                        let current = serde_json::Value::Object(profile.to_dict());
                        crate::meaningful_play::meaningful_profile_hash(&saved)
                            != crate::meaningful_play::meaningful_profile_hash(&current)
                    })
                    .unwrap_or(false)
            };
            if changed {
                self.services
                    .cloud
                    .mark_meaningful_play(&save_name, MeaningfulPlayReason::ChangedSave);
            }
            if let Err(e) = profile.save() {
                log::error!("Could not save the profile: {e}");
            }
        }
    }

    /// Record a durable gameplay event for this career's next accepted upload.
    pub fn mark_meaningful_play(&self, reason: MeaningfulPlayReason) {
        if self.school_sandbox || self.playtest_sandbox {
            return;
        }
        if let Some(profile) = &self.profile {
            self.services
                .cloud
                .mark_meaningful_play(&profile.name, reason);
        }
    }

    pub fn apply_volumes(&mut self) {
        let update = VolumeUpdate::default()
            .master(self.settings.master_volume)
            .sfx(self.settings.sfx_volume)
            .music(self.settings.music_volume)
            .weather(self.settings.weather_volume)
            .engine(self.settings.engine_volume)
            .ui(self.settings.ui_volume);
        self.audio.set_volumes(&update);
        self.audio
            .set_engine_voice(self.settings.engine_voice == "classic");
        self.audio
            .set_jake_voice(self.settings.jake_voice == "classic");
    }

    /// Reflect the Discord presence setting (e.g. after a settings change).
    pub fn apply_presence(&mut self) {
        let enabled = self.online_enabled(self.settings.discord_presence);
        self.services.presence.set_enabled(enabled);
    }

    /// Reflect the drivers-board setting (e.g. after a settings change).
    pub fn apply_online_presence(&mut self) {
        let enabled = self.online_enabled(self.settings.online_presence)
            && !self.settings.profile_sharing_pending_off;
        self.services.online.set_enabled(enabled);
        self.services.journal.set_enabled(enabled);
    }

    /// Reflect the on/off duty notice setting (e.g. after a settings change).
    /// The list is public, so this needs online services on and nothing else.
    pub fn apply_duty_notifications(&mut self) {
        let enabled = self.online_enabled(self.settings.duty_notifications);
        self.services.duty.set_enabled(enabled);
    }

    /// Reflect the cloud backup setting (e.g. after a settings change).
    pub fn apply_cloud_saves(&mut self) {
        let enabled = self.online_enabled(self.settings.cloud_saves);
        self.services.cloud.set_enabled(enabled);
        self.services
            .cloud
            .set_backup_announcements(BackupAnnouncements::from_setting(
                &self.settings.backup_announcements,
            ));
    }

    /// The backup service, for the Cloud backup menu.
    pub fn cloud_saves_service(&self) -> &CloudSaves {
        &self.services.cloud
    }

    /// Adopt freshly confirmed account credentials (setup flow). The drivers
    /// board and cloud backup share them.
    pub fn adopt_online_identity(&mut self, identity: Option<OnlineIdentity>) {
        self.services.online.set_identity(identity.clone());
        self.services.duty.set_identity(identity.as_ref());
        self.services.cloud.set_identity(identity.clone());
        self.services.journal.set_identity(identity);
    }

    /// Reflect the controller setting (e.g. after a settings change).
    pub fn apply_controller(&mut self) {
        self.controller
            .set_enabled(self.settings.controller_enabled);
    }

    /// Reflect the haptics setting (e.g. after a settings change).
    pub fn apply_haptics(&mut self) {
        self.controller
            .set_haptics_enabled(self.settings.haptics_enabled);
    }

    /// Rebuild the control table from the settings (after the player moves
    /// a shortcut, or resets them).
    pub fn apply_bindings(&mut self) {
        self.bindings = KeyBindings::from_settings(&self.settings);
    }

    /// The key or button `action` is on, for whichever device is in use:
    /// the pad button when a controller is active and the control has one,
    /// else the keyboard key (the keyboard always stays active).
    pub fn control_name(&self, action: crate::bindings::Action) -> String {
        let device = self.controller.device();
        if device == ff_core::input_hints::CONTROLLER && action.on_pad() {
            self.bindings.pad_spoken(action)
        } else if device == ff_core::input_hints::TOUCH {
            self.bindings.touch_noun(action)
        } else {
            self.bindings.spoken(action)
        }
    }

    /// Name a control for a spoken prompt, following the active device and
    /// whatever key or button the player has it on.
    pub fn control_hint(&self, action: &str) -> String {
        let device = self.controller.device();
        let moved = if device == ff_core::input_hints::CONTROLLER {
            self.bindings.hint_pad_phrase(action)
        } else if device == ff_core::input_hints::TOUCH {
            self.bindings.hint_touch_phrase(action)
        } else {
            self.bindings.hint_key_phrase(action)
        };
        moved.unwrap_or_else(|| self.controller.hint(action))
    }

    pub fn apply_speech(&mut self) {
        apply_speech_settings(self.speech.as_mut(), &mut self.settings);
    }

    /// Put `text` on the system clipboard; true when it landed.
    pub fn write_clipboard_text(&mut self, text: &str) -> bool {
        self.clipboard.set_text(text)
    }

    // -- music --------------------------------------------------------------------------

    /// Advance a session-local music pool without immediate repeats.
    pub fn next_music_track(&mut self, pool_name: &str, sequence: &[&str]) -> String {
        if sequence.is_empty() {
            return String::new();
        }
        if sequence.len() == 1 {
            let track = sequence[0].to_string();
            self.music_pool_last
                .insert(pool_name.to_string(), track.clone());
            return track;
        }
        let key = (
            pool_name.to_string(),
            sequence.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        );
        let previous = self.music_pool_positions.get(&key).copied();
        let mut index = previous.map_or(0, |p| (p + 1) % sequence.len());
        if Some(sequence[index]) == self.music_pool_last.get(pool_name).map(String::as_str) {
            index = (index + 1) % sequence.len();
        }
        self.music_pool_positions.insert(key, index);
        let track = sequence[index].to_string();
        self.music_pool_last
            .insert(pool_name.to_string(), track.clone());
        track
    }

    /// Play or refresh a pool without jarring compatible menu restarts
    /// (`play_music_sequence(pool, sequence, fade_ms=1500, advance=False)`).
    pub fn play_music_sequence(&mut self, pool_name: &str, sequence: &[&str]) -> String {
        self.play_music_sequence_with(pool_name, sequence, 1500, false)
    }

    pub fn play_music_sequence_with(
        &mut self,
        pool_name: &str,
        sequence: &[&str],
        fade_ms: u32,
        advance: bool,
    ) -> String {
        let synth = self.settings.synth_music;
        if !advance && self.music_rotation_synth == synth {
            if let (Some((pool, _)), Some(track)) =
                (&self.music_rotation_pool, &self.music_rotation_track)
            {
                if pool == pool_name {
                    let track = track.clone();
                    self.music_rotation_pool = Some((
                        pool_name.to_string(),
                        sequence.iter().map(|s| s.to_string()).collect(),
                    ));
                    return track;
                }
            }
        }
        let effective = self.effective_sequence(pool_name, sequence);
        let effective_refs: Vec<&str> = effective.iter().map(String::as_str).collect();
        let picked = self.next_music_track(pool_name, &effective_refs);
        if picked.is_empty() {
            self.clear_music_rotation();
            return picked;
        }
        let track = self.resolve_synth(&picked);
        // Queue the piece after this one so it is ready when this ends.
        if let Some(pos) = effective.iter().position(|k| *k == picked) {
            if let Some(next) = effective.get((pos + 1) % effective.len()) {
                self.request_synth(next);
            }
        }
        self.music_rotation_pool = Some((
            pool_name.to_string(),
            sequence.iter().map(|s| s.to_string()).collect(),
        ));
        self.music_rotation_synth = synth;
        self.music_rotation_track = Some(track.clone());
        self.music_rotation_elapsed_s = 0.0;
        self.audio.play_music_with(&track, fade_ms);
        // Once per track: a synth length is a full composition, too dear per frame.
        self.music_rotation_duration_s = self.track_duration_s(&track);
        track
    }

    /// Advance music beds when their one-shot playback ends.
    pub fn update_music_rotation(&mut self, dt: f64) {
        // Every state's per-frame update lands here, which makes it the one
        // place the live diesel price is sure to be kept current.
        self.sync_fuel_prices();
        let (Some(_), Some(_)) = (&self.music_rotation_pool, &self.music_rotation_track) else {
            // No menu bed is rotating. A drive sitting under this menu
            // (pause, settings, a traffic stop...) keeps its own playlist
            // turning over, so the music does not fall silent when the
            // current track ends.
            self.tick_covered_music(dt);
            return;
        };
        self.music_rotation_elapsed_s += dt.max(0.0);
        if self.music_rotation_elapsed_s < self.music_rotation_duration_s {
            return;
        }
        let (pool_name, sequence) = self.music_rotation_pool.clone().expect("checked above");
        let refs: Vec<&str> = sequence.iter().map(String::as_str).collect();
        self.play_music_sequence_with(&pool_name, &refs, 1500, true);
    }

    fn tick_covered_music(&mut self, dt: f64) {
        let n = self.stack.len();
        if n < 2 {
            return;
        }
        let target = self.stack[..n - 1]
            .iter()
            .rev()
            .map(|entry| Rc::clone(&entry.state))
            .find(|state| state.try_borrow().is_ok_and(|s| s.ticks_covered_music()));
        if let Some(state) = target {
            if let Ok(mut s) = state.try_borrow_mut() {
                s.tick_covered_music(self, dt);
            }
        }
    }

    pub fn clear_music_rotation(&mut self) {
        self.music_rotation_pool = None;
        self.music_rotation_track = None;
        self.music_rotation_elapsed_s = 0.0;
    }

    /// The track the menu rotation is playing, if any.
    pub fn music_rotation_track(&self) -> Option<&str> {
        self.music_rotation_track.as_deref()
    }

    // -- achievements ----------------------------------------------------------------------

    /// `award_achievement(id)` with the defaults (`interrupt=False`,
    /// `announce=True`).
    pub fn award_achievement(&mut self, achievement_id: &str) -> Option<AchievementAward> {
        // What the co-driver does at the wheel or at a stop is not the
        // player's to earn.
        if self.co_driver_away {
            return None;
        }
        self.award_achievement_with(achievement_id, false, true)
            .map(|outcome| outcome.award)
    }

    pub fn award_achievement_with(
        &mut self,
        achievement_id: &str,
        interrupt: bool,
        announce: bool,
    ) -> Option<AwardedAchievement> {
        let result = award(self.profile.as_mut()?, achievement_id)?;
        // Through the guard: a sandboxed session's achievements evaporate
        // with the rest of the run instead of leaking to disk.
        self.save_profile();
        let earned_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let achievement = result.achievement;
        // Sandboxed sessions keep their throwaway career progress isolated
        // from both the installation ledger and public sharing.
        let publicly_eligible = if self.school_sandbox || self.playtest_sandbox {
            false
        } else {
            match self
                .account_achievements
                .record(achievement.id, Some(earned_at_ms))
            {
                Ok(is_new_to_account) => is_new_to_account,
                Err(error) => {
                    // Do not post an achievement that failed to enter the
                    // durable ledger: a later retry could duplicate it.
                    log::error!(
                        "Could not record account achievement {:?}: {error}",
                        achievement.id
                    );
                    false
                }
            }
        };
        if publicly_eligible
            && queue_achievement(
                &self.services.journal,
                achievement.id,
                achievement.name,
                achievement.description,
                earned_at_ms,
            )
        {
            self.services.journal.flush_async();
        }
        self.achievement_notice = result.message.normal.clone();
        self.achievement_notice_timer = 12.0;
        if !announce {
            return Some(AwardedAchievement {
                award: result,
                publicly_eligible,
            });
        }
        self.audio.play_with("ui/level_up", 0.8, 0.0);
        // Live, an achievement is its earcon and its name; the flavor prose
        // is never read at speed (research doc R9). The full record still
        // reaches the review log, and the achievements menu keeps it for
        // good.
        self.say_with(
            achievement_announced(achievement.name),
            Say::new().interrupt(interrupt).review(false),
        );
        self.message_log
            .add(&result.message.normal, MessageCategory::General);
        Some(AwardedAchievement {
            award: result,
            publicly_eligible,
        })
    }
}

impl LeverContext for GameContext {
    fn world(&self) -> &World {
        self.world
    }

    /// The levers only run once a career is loaded; calling this with no
    /// profile is a programming error, as `ctx.profile.x` on `None` was.
    fn profile_mut(&mut self) -> &mut Profile {
        self.profile
            .as_mut()
            .expect("playtest levers run with a loaded profile")
    }

    fn set_playtest_sandbox(&mut self, sandbox: bool) {
        self.playtest_sandbox = sandbox;
    }

    fn save_profile(&mut self) {
        GameContext::save_profile(self);
    }
}
