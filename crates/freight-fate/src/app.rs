//! Application shell: the window, the state stack, and shared services
//! (port of `freight_fate/app.py`, with `__init__.py`'s version lookup and
//! `__main__.py`'s entry point).
//!
//! * [`GameContext`] (`app::context`) -- the services every state gets, and
//!   the state stack itself.
//! * `app::speech_delivery` -- `say` / `say_event`, the ladder, the pacer,
//!   the duck, the transcript.
//! * `app::sdl_shell` -- the SDL window, event translation, clipboard.
//! * `app::logging` -- session log configuration.
//! * `app::testing` -- the headless test rig later state ports reuse.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use ff_core::assets_pack::prefetch_default as prefetch_sound_pack;
use ff_core::data::world::get_world;
use ff_core::message_log::MessageLog;
use ff_core::models::economy::Economy;
use ff_core::models::profile::{self as profile_module, data_dir};
use ff_core::settings::Settings;

use crate::account_achievements::AccountAchievements;
use crate::audio::{Audio, AudioEngine, BassBackend, NullBackend};
use crate::cloud_saves::{BackupAnnouncements, CloudSaves, CloudSavesOptions};
use crate::controller::ControllerManager;
use crate::discord_presence::{DiscordPresence, DiscordPresenceOptions};
use crate::duty_watch::{DutyWatch, DutyWatchOptions};
use crate::jaws_script::JawsScript;
use crate::online_journal::JournalOutbox;
use crate::online_presence::{IdentityStore, OnlinePresence, OnlinePresenceOptions};
use crate::speech::{NullSpeech, SpeechSink};
use crate::states::base::{InputEvent, Key, Mods, State};
use crate::states::driving::DrivingState;
use crate::states::main_menu::{ConfirmQuitState, TouchPracticeState};
use crate::touch::Gesture;

pub mod boot_timing;
pub mod context;
pub mod held_keys;
pub mod key_probe;
pub mod logging;
pub mod sdl_shell;
pub mod speech_delivery;
pub mod synth_music;
pub mod testing;
#[cfg(any(windows, test))]
mod win_keys;

pub use context::{
    share, Clipboard, ContextParts, GameContext, MemoryClipboard, Services, SharedState,
};
pub use held_keys::HeldKeys;
use key_probe::KeyProbe;
pub use logging::{active_log_path, configure_logging};
pub use speech_delivery::{IntoSpoken, Say, SayEvent, Spoken, TRANSCRIPT_TARGET};

use sdl_shell::SdlShell;

pub const WINDOW_SIZE: (u32, u32) = (900, 640);
pub const FPS: u32 = 60;
pub const BG_COLOR: (u8, u8, u8) = (12, 12, 16);
pub const TEXT_COLOR: (u8, u8, u8) = (235, 235, 225);
pub const HILIGHT_COLOR: (u8, u8, u8) = (255, 210, 90);

/// The game's version (`freight_fate.__version__`): the version
/// `tools/build_release.py` stamped into `build_info.json` beside the
/// executable when there is one, else this crate's.
pub fn version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| baked_version().unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string()))
}

/// `package_version` from the `build_info.json` next to the executable.
fn baked_version() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let resource_dir =
        ff_core::data::data_resources::resource_dir_for_executable(&exe, cfg!(target_os = "macos"));
    let info = std::fs::read_to_string(resource_dir.join("build_info.json")).ok()?;
    let data: serde_json::Value = serde_json::from_str(&info).ok()?;
    data.get("package_version")?
        .as_str()
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// `pygame.time.Clock`: `tick(fps)` blocks to cap the frame rate and
/// returns the seconds since the previous tick.
pub struct FrameClock {
    last: Instant,
    /// Whether `tick` sleeps to hold the frame rate (a harness driving
    /// `frame()` itself never calls `tick`).
    pub limit: bool,
}

impl FrameClock {
    pub fn new(limit: bool) -> Self {
        Self {
            last: Instant::now(),
            limit,
        }
    }

    pub fn tick(&mut self, fps: u32) -> f64 {
        if self.limit {
            let frame = Duration::from_secs_f64(1.0 / fps as f64);
            let elapsed = self.last.elapsed();
            if elapsed < frame {
                std::thread::sleep(frame - elapsed);
            }
        }
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        dt
    }
}

/// What `App::run` pushes first: `MainMenuState` once `states::main_menu`
/// is ported; until then a one-row placeholder that quits, so `--smoke` and
/// the headless loop have a screen to stand on.
pub type InitialState = Box<dyn FnOnce(&mut GameContext) -> SharedState>;

fn placeholder_main_menu(ctx: &mut GameContext) -> SharedState {
    use crate::states::assist_picker::AssistPickerState;
    // The one-time Driving assistance picker goes first until it is answered.
    if AssistPickerState::is_owed(ctx) {
        return share(AssistPickerState::new());
    }
    share(crate::states::main_menu::MainMenuState::new())
}

pub struct App {
    pub ctx: GameContext,
    shell: Option<SdlShell>,
    clock: FrameClock,
    initial_state: Option<InitialState>,
    // Tooling may stage a small number of deliberate player inputs here. They
    // join the SDL batch in `frame`, so they retain the normal held-key,
    // controller, speech and state-dispatch behavior.
    queued_player_input: Vec<InputEvent>,
    /// Agent sessions: the operator's keyboard never reaches the game. The
    /// window is minimized at launch, and still the owner's typing in the
    /// next window arrived as Space readouts and an S limit report mid-run
    /// (2026-09-01); the agent's own keys come in through
    /// `queue_player_input`, which this never touches.
    operator_keys_ignored: bool,
    /// Set by an input policy for one frame: the frame runs with no time
    /// passing. The agent server's lockstep uses it so the road waits while
    /// the agent decides.
    world_held: bool,
    /// The key probe, while one is running (see `app::key_probe`).
    key_probe: Option<KeyProbe>,
    /// A two- or three-finger touch hold resolved through the live keyboard
    /// bindings. Pedal holds live in `TouchInput`; these actions can be rebound.
    touch_bound_hold: Option<crate::bindings::Chord>,
}

/// Read-only driving facts available to a normal-input policy.
///
/// This deliberately contains values the observer may use to decide its next
/// key, rather than the driving state itself. A policy cannot reach physics,
/// assists, or the state stack through this snapshot.
#[derive(Debug, Clone, Copy)]
pub struct DrivingObservation {
    pub position_mi: f64,
    pub air_ready: bool,
    pub parking_brake: bool,
    pub speed_control_armed: bool,
    pub keeper_active: bool,
    pub cruise_active: bool,
    pub departure_chain: bool,
    pub hazard_active: bool,
    pub pull_over_active: bool,
    pub lane_keeping_full: bool,
    pub off_pavement: bool,
    pub truck_damage_pct: f64,
    pub cargo_damage_pct: f64,
    pub speed_mph: f64,
    /// The limit enforcement is holding the truck to, once one has been
    /// read off the road (None on a drive's first frames).
    pub speed_limit_mph: Option<f64>,
    /// Adaptive cruise's set point, when it is engaged.
    pub cruise_set_mph: Option<f64>,
    /// The speed keeper's hold, when it has the zone.
    pub keeper_mph: Option<f64>,
}

/// The only capability a windowed input policy receives for one frame.
///
/// It can inspect a value snapshot, queue the same input event a player can
/// make, and ask the existing event pacer whether player-facing driving
/// speech is still draining. It cannot mutate application or driving state.
pub struct PlayerInputFrame<'a> {
    app: &'a mut App,
}

fn staged_road_handoff(description: &str, start_mi: f64) -> String {
    format!("Staged: {description}. You take the wheel {start_mi:.1} miles up the road.")
}

impl PlayerInputFrame<'_> {
    pub fn driving_observation(&self) -> Option<DrivingObservation> {
        self.app.driving_observation()
    }

    /// The player's own shortcut table, for a tool call that names a
    /// control rather than a key.
    pub fn bindings(&self) -> &crate::bindings::KeyBindings {
        &self.app.ctx.bindings
    }

    pub fn queue_player_input(&mut self, event: InputEvent) {
        self.app.queue_player_input(event);
    }

    pub fn event_speech_busy(&mut self) -> bool {
        self.app.ctx.event_voice_busy()
    }

    /// Run this frame with no time passing: events still pump and queued
    /// input still lands, but nothing in the world moves.
    pub fn hold_world(&mut self) {
        self.app.world_held = true;
    }

    /// The agent server's `key_probe` tool: start recording what the
    /// keyboard delivers, or stop and report it (see `app::key_probe`).
    pub fn key_probe(&mut self, start: bool) -> String {
        if start {
            self.app.start_key_probe();
            let shut_out = self.app.operator_keys_ignored;
            return format!(
                "Key probe recording.{} Have the operator hold the arrow keys for several \
                 seconds, then call key_probe with action report. Do not press keys \
                 yourself meanwhile: they would be measured too.",
                if shut_out {
                    " The operator's keyboard is shut out, so call operator_keys live true first."
                } else {
                    ""
                }
            );
        }
        match self.app.stop_key_probe() {
            Some(report) => report.text(),
            None => "No key probe is running. Call key_probe with action start.".to_string(),
        }
    }

    /// Whether this frame will run with no time passing.
    pub fn world_is_held(&self) -> bool {
        self.app.world_held
    }

    /// Hand the keyboard to the operator (`live`) or take it back: the
    /// window is restored and raised so their keys land, or minimized and
    /// the keys dropped at the door again. Used by the agent server's
    /// `operator_keys` tool, at the owner's request only.
    pub fn set_operator_keys(&mut self, live: bool) -> String {
        if live {
            self.app.allow_operator_keys();
            self.app.restore_window();
            "Operator keys are live: the game window is up and the keyboard reaches the \
             game. Anything typed elsewhere while it has focus is truck input."
                .to_string()
        } else {
            self.app.ignore_operator_keys();
            self.app.minimize_window();
            "Operator keys are off: the window is minimized and the keyboard is dropped \
             at the door."
                .to_string()
        }
    }

    /// The rows of the menu on screen and which has focus, read the same
    /// way the playtest harness reads them (off the rendered lines), or
    /// `None` when the current screen is not a menu. A value snapshot, in
    /// the same spirit as [`PlayerInputFrame::driving_observation`].
    pub fn menu_rows(&self) -> Option<(Vec<String>, usize)> {
        let state = self.app.state()?;
        let state = state.borrow();
        crate::playtest::menu::menu_rows(&*state, &self.app.ctx)
    }

    /// Stage a drive at a discovered road feature -- the same lever
    /// `--playtest-road --find` pulls, exposed so an agent session can
    /// start where testing is needed instead of menuing its way there.
    ///
    /// This deliberately exceeds the "cannot mutate application state"
    /// contract the observer policy lives under: it is scenario staging,
    /// not play, and it swaps the screen for a freshly built drive the
    /// same way the road launcher does at startup. The staged career is
    /// whatever data dir the process runs in -- for the agent server,
    /// always the audited sandbox.
    pub fn stage_road_hit(
        &mut self,
        hit: &crate::playtest::road::Hit,
        opts: &crate::playtest::road::RoadOptions,
    ) -> Result<String, String> {
        use crate::playtest::road;
        let description = hit.describe();
        let (driving, start_mi) = road::build_driving(&mut self.app.ctx, hit, opts);
        self.app.push_state(driving);
        Ok(staged_road_handoff(&description, start_mi))
    }

    /// Put the sandbox career in the situation asked for and reopen the
    /// terminal on it, whatever screen was up. Scenario staging like
    /// `stage_road_hit`: it swaps the whole screen stack for a fresh
    /// terminal, so the board, garage and clock read the new career.
    pub fn stage_scenario(
        &mut self,
        scenario: &crate::playtest::scenario::Scenario,
    ) -> Result<String, String> {
        use crate::states::city::CityMenuState;
        let notes = crate::playtest::scenario::apply(&mut self.app.ctx, scenario)?;
        // Down to the title screen, never past it: an empty stack is how the
        // loop knows the game is over, and the first live scenario quit the
        // game on the frame it was staged (2026-09-12). The terminal then
        // sits on the title screen exactly as a normal continue leaves it.
        while self.app.ctx.stack_len() > 1 {
            self.app.ctx.pop_state();
            self.app.ctx.run_deferred();
        }
        let city = CityMenuState::new(&self.app.ctx, true);
        self.app.push_state(city);
        self.app.ctx.run_deferred();
        let profile = self
            .app
            .ctx
            .profile
            .as_ref()
            .expect("apply ensured a career");
        let where_now = self
            .app
            .ctx
            .world
            .spoken_city(&profile.current_city, Some(true));
        let mut text = format!(
            "Scenario staged: {} at {} terminal, level {}, {} deliveries, {} dollars, {}.",
            profile.name,
            ff_core::speech_text::the_city(&where_now),
            profile.career.level(),
            profile.career.deliveries,
            ff_core::pyfmt::fmt_grouped(profile.money(), 0),
            profile.business_status,
        );
        if !notes.is_empty() {
            text.push(' ');
            text.push_str(&notes.join(" "));
        }
        text.push_str(" The terminal menu is open; listen for it.");
        Ok(text)
    }

    /// Keep a policy-held key held, without re-dispatching a key event.
    ///
    /// The focus-lost handler wipes the held-key store as a safety measure
    /// for real keyboards -- the OS stops delivering KeyUp to an unfocused
    /// window, so a wipe beats a stuck pedal. A policy's holds arrive
    /// through [`PlayerInputFrame::queue_player_input`], not the OS, so on
    /// a desktop where focus bounces (a screen reader working other
    /// windows) that same wipe silently released the agent's throttle.
    /// The policy re-asserts its holds every frame with this; only the
    /// held-state store is touched, so per-press handlers never re-fire.
    pub fn assert_held(&mut self, key: Key) {
        self.app.ctx.input.press(key, Mods::NONE);
    }

    /// Whether the drive would read `key` as down this frame -- physically
    /// or through the held-key tracker's screen-reader pulse. An inspector
    /// for the agent server's tests, not a control.
    pub fn key_reads_pressed(&self, key: Key) -> bool {
        self.app.ctx.input.is_pressed(key)
    }

    /// How many screens are stacked, and whether the game still means to
    /// run. An inspector for the agent server's tests: staging must leave
    /// the title screen under the terminal, or the loop reads the empty
    /// stack as the game being over.
    pub fn screen_depth(&self) -> (usize, bool) {
        (self.app.ctx.stack_len(), self.app.ctx.running)
    }
}

impl App {
    /// The windowed game: SDL, the vendored sound pack prefetch, speech via
    /// Prism, BASS audio, every online service, the controller subsystem.
    pub fn new() -> Result<App, String> {
        // Opt PS4/PS5 pads into HIDAPI rumble so their motors work like Xbox
        // pads. Must be set before SDL init; Xbox/XInput needs no flag.
        for (name, value) in [
            ("SDL_JOYSTICK_HIDAPI_PS4_RUMBLE", "1"),
            ("SDL_JOYSTICK_HIDAPI_PS5_RUMBLE", "1"),
        ] {
            if std::env::var_os(name).is_none() {
                std::env::set_var(name, value);
            }
        }
        if std::env::var_os("FREIGHT_FATE_NO_SPEECH").is_some_and(|v| !v.is_empty()) {
            std::env::set_var("SDL_VIDEODRIVER", "dummy");
            std::env::set_var("SDL_AUDIODRIVER", "dummy");
        }
        // Kick the ~225MB sound pack's read-and-unmask onto a background
        // thread before anything else: it has no dependency on SDL or the
        // world data that follows, so it overlaps the rest of startup
        // instead of stalling the first sound played.
        prefetch_sound_pack();
        let shell = SdlShell::new(&format!("Freight Fate {}", version()))?;
        boot_timing::mark("window");
        // Prism on its own worker thread: a wedged screen-reader or SAPI
        // call costs sentences, never the drive (a synchronous call here
        // froze the whole game at an I-77 merge -- Shane, 2026-08-30).
        let speech: Box<dyn SpeechSink> = Box::new(crate::speech::ThreadedSpeech::spawn());
        boot_timing::mark("speech");
        // Deferred: the output-device open runs on a worker thread, so a
        // machine where the probe hangs boots reading input instead of deaf.
        let audio: Box<dyn Audio> = Box::new(AudioEngine::new_deferred());
        boot_timing::mark("audio");
        Ok(Self::build(Some(shell), speech, audio))
    }

    /// The headless app: no window, no SDL, the null audio backend, the
    /// given speech sink (a `CaptureSpeech` in tests and the harness, a
    /// `NullSpeech` for `--headless`), the same `GameContext` otherwise.
    pub fn new_headless(speech: Box<dyn SpeechSink>) -> App {
        let audio: Box<dyn Audio> =
            Box::new(AudioEngine::with_backend(Box::new(NullBackend::new())));
        Self::new_headless_with(speech, audio)
    }

    /// A headless app on a caller-built audio engine (BASS on the no-sound
    /// device, a recorder, ...).
    pub fn new_headless_with(speech: Box<dyn SpeechSink>, audio: Box<dyn Audio>) -> App {
        Self::build(None, speech, audio)
    }

    fn build(shell: Option<SdlShell>, speech: Box<dyn SpeechSink>, audio: Box<dyn Audio>) -> App {
        // The restored 1.5 classics: compiled in, so they must exist before
        // the title screen's first menu theme, not just before a drive.
        // Every GameContext is built here, so this one call covers drives too.
        crate::audio::classic_music::register();
        let settings = Settings::load();
        sdl_shell::set_touch_haptics(settings.touch_haptics);
        boot_timing::mark("settings");
        // Stations players suggested and the owner accepted; the first drive
        // reads whatever copy is saved, so this never holds up launch. A
        // windowless run (the smoke check, the tools) has no dial to fill.
        if shell.is_some() {
            crate::community_stations::refresh_in_background(&settings);
        }
        let message_log = MessageLog::new();
        let world = get_world();
        boot_timing::mark("world");
        let economy = Economy::default();
        let presence = DiscordPresence::new(DiscordPresenceOptions {
            enabled: settings.discord_presence,
            ..Default::default()
        });
        boot_timing::mark("discord presence");
        // identity is loaded unconditionally, not gated on whether any
        // online setting is currently on: OnlinePresence/CloudSaves.
        // set_enabled() both refuse to turn on without an identity already
        // in hand, and nothing re-loads it when a player flips a setting on
        // mid-session -- only the account-link flow (adopt_online_identity)
        // does that. A player who linked an account and then turned every
        // online setting off must still be able to turn one back on later
        // without re-pasting credentials.
        let data_dir = data_dir();
        // Account-achievement migration is deliberately before service setup:
        // importing existing careers must stay silent and cannot queue an
        // online action. A completed version is persisted in the ledger so
        // future starts only load it.
        let mut account_achievements = AccountAchievements::load(&data_dir);
        if let Err(error) = account_achievements.migrate_local_profiles() {
            log::error!("Could not migrate local account achievements: {error}");
        }
        boot_timing::mark("account achievements");
        let store = IdentityStore::platform(&data_dir);
        // May wait up to a few seconds on the Mac keychain; a hung ACL
        // prompt must not freeze launch with no further log (issue 266).
        let identity = store.load();
        boot_timing::mark("driver identity");
        let online = OnlinePresence::new(OnlinePresenceOptions {
            enabled: settings.online_presence,
            identity: identity.clone(),
            ..Default::default()
        });
        let duty = DutyWatch::new(DutyWatchOptions {
            enabled: settings.online_services && settings.duty_notifications,
            own_driver_id: identity.as_ref().map(|i| i.driver_id.clone()),
            ..Default::default()
        });
        let cloud = CloudSaves::new(CloudSavesOptions {
            enabled: settings.cloud_saves,
            identity: identity.clone(),
            data_dir: data_dir.clone(),
            backup_announcements: BackupAnnouncements::from_setting(&settings.backup_announcements),
            ..Default::default()
        });
        let journal = JournalOutbox::new(
            identity.clone(),
            settings.online_presence,
            &store.path().with_file_name("online-outbox.json"),
        );
        // Mastodon shares ride the same durable-outbox machinery but keep
        // their own file and enabled flag: posting to the player's own
        // Mastodon account is a separate consent from public Profile
        // sharing.
        let mastodon = JournalOutbox::new(
            identity,
            settings.mastodon_sharing,
            &store.path().with_file_name("online-mastodon-outbox.json"),
        );
        // Every profile save, wherever it happens, queues a cloud backup.
        let backup = cloud.clone();
        profile_module::set_save_listener(Some(Arc::new(
            move |profile: &profile_module::Profile| {
                backup.queue_backup(&profile.name, serde_json::Value::Object(profile.to_dict()));
            },
        )));
        boot_timing::mark("online services");
        let pads_allowed = crate::controller::pads_allowed(
            settings.controller_enabled,
            std::env::var_os(crate::controller::NO_CONTROLLER_ENV).as_deref(),
        );
        let controller = match &shell {
            Some(shell) => ControllerManager::new(
                pads_allowed,
                settings.haptics_enabled,
                Some(crate::controller::sdl::sdl_factory(shell.sdl.clone())),
            ),
            None => ControllerManager::detached(pads_allowed, settings.haptics_enabled),
        };
        boot_timing::mark("controller");
        let clipboard: Box<dyn Clipboard> = match &shell {
            Some(shell) => Box::new(shell.clipboard()),
            None => Box::new(MemoryClipboard::default()),
        };
        let mut ctx = GameContext::new(ContextParts {
            speech,
            audio,
            controller,
            settings,
            world,
            economy,
            account_achievements,
            message_log,
            clipboard,
            services: Services {
                presence,
                online,
                duty,
                jaws_script: JawsScript::new(),
                cloud,
                journal,
                mastodon,
            },
        });
        ctx.apply_volumes();
        ctx.apply_speech();
        boot_timing::mark("volumes and voice");
        App {
            ctx,
            shell,
            clock: FrameClock::new(true),
            initial_state: None,
            queued_player_input: Vec::new(),
            operator_keys_ignored: false,
            world_held: false,
            key_probe: None,
            touch_bound_hold: None,
        }
    }

    /// Start recording what the keyboard delivers (see `app::key_probe`).
    pub fn start_key_probe(&mut self) {
        self.key_probe = Some(KeyProbe::new(&self.ctx.input));
    }

    /// Stop the probe and hand back its report, if one was running.
    pub fn stop_key_probe(&mut self) -> Option<key_probe::Report> {
        self.key_probe.take().map(|probe| probe.report())
    }

    /// `freightfate --key-probe`: an inert screen, the probe on it, for
    /// `seconds` or until Escape. No services start, no career is touched.
    pub fn run_key_probe(&mut self, seconds: u64) -> key_probe::Report {
        self.ctx.running = true;
        self.push_shared(share(key_probe::KeyProbeState::new(seconds)));
        self.start_key_probe();
        while self.ctx.running {
            let dt = self.clock.tick(FPS);
            self.frame(dt);
            let done = self.key_probe.as_ref().is_none_or(|probe| {
                probe.escape_seen() || probe.elapsed_ms(&self.ctx.input) >= seconds * 1000
            });
            if done {
                break;
            }
        }
        self.stop_key_probe()
            .unwrap_or_else(|| KeyProbe::new(&self.ctx.input).report())
    }

    /// Choose the screen `run` starts on (the main menu, once ported).
    pub fn set_initial_state(&mut self, factory: InitialState) {
        self.initial_state = Some(factory);
    }

    pub fn is_headless(&self) -> bool {
        self.shell.is_none()
    }

    // -- state stack (forwarders; the stack lives on the context) ----------------------

    pub fn state(&self) -> Option<SharedState> {
        self.ctx.state()
    }

    pub fn states(&self) -> Vec<SharedState> {
        self.ctx.states()
    }

    /// Minimize the game window (see [`SdlShell::minimize`]); a no-op when
    /// headless. The agent server calls this so the operator's keyboard can
    /// never land in the game.
    pub fn minimize_window(&mut self) {
        if let Some(shell) = self.shell.as_mut() {
            shell.minimize();
        }
    }

    /// Undo [`App::minimize_window`]: the window comes back and asks for
    /// focus; a no-op when headless.
    pub fn restore_window(&mut self) {
        if let Some(shell) = self.shell.as_mut() {
            shell.restore();
        }
    }

    pub fn running(&self) -> bool {
        self.ctx.running
    }

    pub fn set_running(&mut self, running: bool) {
        self.ctx.running = running;
    }

    fn queue_player_input(&mut self, event: InputEvent) {
        self.queued_player_input.push(event);
    }

    /// Drop every key the operating system delivers from here on; only
    /// [`PlayerInputFrame::queue_player_input`] can press a key. Focus and
    /// quit events still pass.
    pub fn ignore_operator_keys(&mut self) {
        self.operator_keys_ignored = true;
    }

    /// Undo [`App::ignore_operator_keys`]: the operating system's keys count
    /// again, so a human can take the wheel alongside an agent.
    pub fn allow_operator_keys(&mut self) {
        self.operator_keys_ignored = false;
    }

    fn driving_observation(&self) -> Option<DrivingObservation> {
        let state = self.state()?;
        let state = state.borrow();
        let drive = state
            .as_any()
            .downcast_ref::<crate::states::driving::DrivingState>()?;
        Some(DrivingObservation {
            position_mi: drive.trip.position_mi,
            air_ready: drive.trip.truck.air_ready(),
            parking_brake: drive.trip.truck.parking_brake,
            speed_control_armed: drive.speed_control_armed,
            keeper_active: drive.keeper_mph.is_some(),
            cruise_active: drive.cruise_mph.is_some(),
            departure_chain: drive.departure_chain,
            hazard_active: drive.hazard_deadline.is_some(),
            pull_over_active: drive.trip.pull_over_active,
            lane_keeping_full: self.ctx.settings.lane_is_automated(),
            off_pavement: drive.off_pavement(),
            truck_damage_pct: drive.trip.truck.damage_pct,
            cargo_damage_pct: drive.trip.truck.cargo_damage_pct,
            speed_mph: drive.trip.truck.speed_mph(),
            speed_limit_mph: drive.enforced_limit_prev,
            cruise_set_mph: drive.cruise_mph,
            keeper_mph: drive.keeper_mph,
        })
    }

    pub fn push_state<S: State + 'static>(&mut self, state: S) {
        self.ctx.push_state(state);
        self.ctx.run_deferred();
    }

    pub fn push_state_with<S: State + 'static>(&mut self, state: S, should_enter: bool) {
        self.ctx.push_state_with(state, should_enter);
        self.ctx.run_deferred();
    }

    pub fn push_shared(&mut self, state: SharedState) {
        self.ctx.push_shared(state);
        self.ctx.run_deferred();
    }

    pub fn pop_state(&mut self) {
        self.ctx.pop_state();
        self.ctx.run_deferred();
    }

    pub fn pop_state_with(&mut self, should_exit: bool, reentry: bool) {
        self.ctx.pop_state_with(should_exit, reentry);
        self.ctx.run_deferred();
    }

    pub fn replace_state<S: State + 'static>(&mut self, state: S) {
        self.ctx.replace_state(state);
        self.ctx.run_deferred();
    }

    pub fn replace_state_with<S: State + 'static>(
        &mut self,
        state: S,
        should_exit: bool,
        reentry: bool,
    ) {
        self.ctx.replace_state_with(state, should_exit, reentry);
        self.ctx.run_deferred();
    }

    pub fn reset_to<S: State + 'static>(&mut self, state: S) {
        self.ctx.reset_to(state);
        self.ctx.run_deferred();
    }

    pub fn reset_to_with<S: State + 'static>(
        &mut self,
        state: S,
        should_exit: bool,
        reentry: bool,
    ) {
        self.ctx.reset_to_with(state, should_exit, reentry);
        self.ctx.run_deferred();
    }

    // -- dispatch ---------------------------------------------------------------------------

    /// Feed a controller event to the manager, then to the active state.
    ///
    /// The manager updates its cached axis/modifier/hot-plug state first and
    /// reports whether the event is an accepted button press for the bound
    /// controller; only those reach the state, so a duplicate from a pad
    /// that enumerates twice can never fire an action a second time.
    pub fn dispatch_controller(&mut self, event: &InputEvent) {
        let forward = self.ctx.controller.process_event(event);
        if forward && self.ctx.controller.active() {
            if let Some(state) = self.ctx.state() {
                state.borrow_mut().handle_controller(&mut self.ctx, event);
                self.ctx.run_deferred();
            }
        }
    }

    /// Hand a keyboard/window event to the active state.
    ///
    /// Message review gets first refusal on every key press, which is what
    /// makes the review controls work on every screen instead of only the
    /// ones that remembered to call `handle_message_review`. A state that
    /// takes typed text declines them itself.
    pub fn dispatch_to_state(&mut self, event: &InputEvent) {
        let Some(state) = self.ctx.state() else {
            return;
        };
        if event.key_down().is_some() {
            self.ctx.controller.note_keyboard();
            let consumed = state
                .borrow_mut()
                .handle_message_review(&mut self.ctx, event);
            if consumed {
                self.ctx.run_deferred();
                return;
            }
        }
        state.borrow_mut().handle_event(&mut self.ctx, event);
        self.ctx.run_deferred();
    }

    /// Hand a touch gesture to the active state, or press its key when the
    /// state leaves it to the keyboard table.
    ///
    /// Spoken hints follow: from here on they name gestures, until a key or
    /// a controller button is pressed.
    pub fn dispatch_gesture(&mut self, gesture: Gesture) {
        self.ctx.controller.note_touch();
        if self
            .ctx
            .state()
            .is_some_and(|state| state.borrow().as_any().is::<TouchPracticeState>())
        {
            let state = self
                .ctx
                .state()
                .expect("the touch practice state is still on top");
            let taken = state.borrow_mut().handle_gesture(&mut self.ctx, gesture);
            self.ctx.run_deferred();
            if taken {
                return;
            }
        }
        if let Some(action) = match gesture {
            Gesture::EmergencyBrakeHoldBegan => Some(crate::bindings::Action::EmergencyBrake),
            Gesture::HornHoldBegan => Some(crate::bindings::Action::Horn),
            _ => None,
        } {
            if let Some(previous) = self.touch_bound_hold.take() {
                self.handle_event(&InputEvent::KeyUp {
                    key: previous.key,
                    mods: previous.mods,
                });
            }
            if let Some(chord) = self.ctx.bindings.chords(action).into_iter().next() {
                self.touch_bound_hold = Some(chord);
                self.handle_event(&InputEvent::KeyDown {
                    key: chord.key,
                    mods: chord.mods,
                    text: None,
                    repeat: false,
                });
            }
            return;
        }
        if gesture == Gesture::HoldEnded {
            if let Some(chord) = self.touch_bound_hold.take() {
                self.handle_event(&InputEvent::KeyUp {
                    key: chord.key,
                    mods: chord.mods,
                });
                return;
            }
        }
        let events = if gesture.held_key().is_some() {
            gesture.hold_events()
        } else {
            if let Some(state) = self.ctx.state() {
                let taken = state.borrow_mut().handle_gesture(&mut self.ctx, gesture);
                self.ctx.run_deferred();
                if taken {
                    return;
                }
            }
            gesture.key_events()
        };
        self.ctx.controller.touch_keys = true;
        for event in events {
            self.handle_event(&event);
        }
        self.ctx.controller.touch_keys = false;
    }

    /// Alt+F4 and the window's close button ask, they do not just go.
    ///
    /// Closing the window used to end the process on the spot. Mid-drive that
    /// is destructive and silent: saving happens only at stops, so the leg
    /// being driven is gone and the save still points at the last stop.
    /// Darren lost two routes to a mis-hit Alt+F4 and asked for the same gate
    /// Escape already puts in front of quitting (2026-08-22).
    ///
    /// The second close request is obeyed without further argument. A
    /// confirmation the player cannot get past would be a worse bug than the
    /// one it fixes -- if speech has dropped, or the dialog is somehow
    /// unreachable, Alt+F4 twice always closes the game.
    pub fn handle_close_request(&mut self) {
        let already_asking = self
            .ctx
            .state()
            .is_some_and(|state| state.borrow().as_any().is::<ConfirmQuitState>());
        if already_asking {
            self.ctx.running = false;
            return;
        }
        let unsaved = self.drive_in_progress();
        self.push_state(ConfirmQuitState::with_unsaved_drive(unsaved));
    }

    /// Whether a leg is being driven right now, saved nowhere.
    pub fn drive_in_progress(&self) -> bool {
        self.ctx
            .states()
            .iter()
            .any(|state| state.borrow().as_any().is::<DrivingState>())
    }

    /// One event through the loop's switch: focus, quit, controller, state.
    pub fn handle_event(&mut self, event: &InputEvent) {
        match event {
            InputEvent::WindowFocusGained => {
                // Switching screen readers happens outside the game;
                // re-check speech the moment the player comes back. The
                // keyboard's repeat timing is re-read for the same reason:
                // a player who changed it gets it without a restart.
                self.ctx.speech.request_refresh();
                self.ctx.input.refresh_repeat_timing();
                // Nothing held before the player left is held now, either.
                self.ctx.input.clear_pulses();
                // ...and then the screen gets it too. Python tests focus
                // with its own `if`, not the `elif` chain, so the event
                // carries on to the state; a `match` arm here quietly ate
                // it, and with it the two screens that wait for the player
                // to come back from a browser (online setup, the Mastodon
                // link) -- both sat silent on return.
                self.dispatch_to_state(event);
            }
            InputEvent::WindowFocusLost => {
                self.ctx.input.clear();
                self.dispatch_to_state(event);
            }
            InputEvent::Quit => self.handle_close_request(),
            InputEvent::Gesture(gesture) => self.dispatch_gesture(*gesture),
            InputEvent::KeyDown { key, mods, .. } => {
                self.ctx.input.press(*key, *mods);
                self.dispatch_to_state(event);
            }
            InputEvent::KeyUp { key, mods } => {
                self.ctx.input.release(*key, *mods);
                self.dispatch_to_state(event);
            }
            event if event.is_controller_event() => self.dispatch_controller(event),
            event => self.dispatch_to_state(event),
        }
    }

    /// The per-frame work after the events: controller repeats, the speech
    /// poll, disconnects, cloud notices, audio fades, the duck, the state
    /// update, presence, the achievement notice.
    pub fn tick(&mut self, dt: f64) {
        // Auto-repeat (held D-pad left/right) and analog smoothing.
        // Synthetic repeats go straight to the state (bypassing the manager,
        // whose press state must not be reset) and only where the menu
        // wants adjust-repeat -- driving keeps D-pad discrete.
        let repeats = self.ctx.controller.tick(dt);
        if let Some(state) = self.ctx.state() {
            if state.borrow().wants_controller_repeat() {
                for event in &repeats {
                    state.borrow_mut().handle_controller(&mut self.ctx, event);
                    self.ctx.run_deferred();
                }
            }
        }
        // Reconnect speech if the player's screen reader changed.
        self.ctx.speech.poll(dt);
        if self.ctx.controller.take_disconnect() {
            self.ctx.say(
                "Controller disconnected. You can keep playing with the \
                 keyboard, or reconnect your controller.",
            );
            if let Some(state) = self.ctx.state() {
                state.borrow_mut().on_controller_disconnect(&mut self.ctx);
                self.ctx.run_deferred();
            }
        }
        // Cloud backup refusals speak wherever the player is -- driving or
        // in menus -- not only on the terminal's Save game item: the worker
        // thread queues them and this loop delivers, queued on the normal
        // announcement channel so a backup notice never cuts off urgent
        // driving speech.
        for line in self.ctx.services.cloud.take_announcements() {
            self.ctx.say_with(line, Say::queued());
        }
        // A reviewed career the server accepted: clear its mark silently so
        // the next backup goes up unmarked. A career that is not loaded
        // clears the next time it is accepted.
        for name in self.ctx.services.cloud.take_absolved() {
            if let Some(profile) = self
                .ctx
                .profile
                .as_mut()
                .filter(|p| crate::cloud_saves::save_slot_name(&p.name) == name)
            {
                profile.absolve();
                self.ctx.save_profile();
            }
        }
        // Another driver setting off or signing off, when the player asked
        // to hear it: same channel, same reason.
        for line in self.ctx.services.duty.take_announcements() {
            self.ctx.say_with(line, Say::queued());
        }
        // The JAWS arrow-key script finishing on its own thread.
        for line in self.ctx.services.jaws_script.take_announcements() {
            self.ctx.say_with(line, Say::queued());
        }
        self.ctx.audio.update(dt); // advance time-based audio fades
        self.ctx.update_speech_duck(); // restore the mix after speech

        // Which calendar the player hears, for every date the career speaks.
        let live_calendar =
            self.ctx.settings.real_weather && self.ctx.settings.live_weather_controls_calendar;
        if let Some(profile) = self.ctx.profile.as_mut() {
            profile.live_calendar = live_calendar;
        }
        if let Some(state) = self.ctx.state() {
            state.borrow_mut().update(&mut self.ctx, dt);
            self.ctx.run_deferred();
            // A screen with no presence of its own (Settings, help, the
            // drivers list over the pause menu) is still wherever the state
            // under it is. Reading the top alone signed a paused driver off
            // the duty board 20 seconds into a sub-screen (2026-09-28).
            let states = self.ctx.states();
            let presence = states
                .iter()
                .rev()
                .find_map(|s| s.try_borrow().ok()?.presence(&self.ctx));
            let online = states
                .iter()
                .rev()
                .find_map(|s| s.try_borrow().ok()?.online_presence(&self.ctx));
            self.ctx.services.presence.update(presence);
            self.ctx.services.online.update(online);
        }
        if self.ctx.achievement_notice_timer > 0.0 {
            self.ctx.achievement_notice_timer = (self.ctx.achievement_notice_timer - dt).max(0.0);
            if self.ctx.achievement_notice_timer == 0.0 {
                self.ctx.achievement_notice.clear();
            }
        }
    }

    /// The lines the window would show this frame: the state's, capped at
    /// 18 (16 + a blank + the achievement notice while one shows).
    pub fn visible_lines(&self) -> Vec<String> {
        let Some(state) = self.ctx.state() else {
            return Vec::new();
        };
        let base = state.borrow().lines(&self.ctx);
        let mut lines: Vec<String> = if self.ctx.achievement_notice.is_empty() {
            base.into_iter().take(18).collect()
        } else {
            let mut lines: Vec<String> = base.into_iter().take(16).collect();
            lines.push(String::new());
            lines.push(self.ctx.achievement_notice.clone());
            lines
        };
        lines.truncate(18);
        lines
    }

    pub fn render(&mut self) {
        let lines = self.visible_lines();
        if let Some(shell) = self.shell.as_mut() {
            shell.render(&lines);
        }
    }

    /// One whole frame: pump events, tick, render. `dt` is the frame time.
    pub fn frame(&mut self, dt: f64) {
        let mut events = match self.shell.as_mut() {
            Some(shell) => match shell.poll() {
                Some(events) => events,
                None => {
                    log::error!(
                        "event pump failed (controller {}); skipping this batch",
                        if self.ctx.controller.connected() {
                            "connected"
                        } else {
                            "disconnected"
                        }
                    );
                    Vec::new()
                }
            },
            None => Vec::new(),
        };
        if self.operator_keys_ignored {
            events = without_operator_keys(events);
        }
        events.append(&mut self.queued_player_input);
        // Clock the held-key tracker before this frame's events: a screen
        // reader's re-injected press-and-release pairs are told apart from a
        // finger by which frame they land in (see `app::held_keys`).
        self.ctx.input.begin_frame(dt);
        self.ctx
            .input
            .set_bridge(self.ctx.speech.backend_name().eq_ignore_ascii_case("jaws"));
        if let Some(probe) = self.key_probe.as_mut() {
            probe.begin_frame(&self.ctx.input);
        }
        for event in &events {
            self.handle_event(event);
            if let Some(probe) = self.key_probe.as_mut() {
                probe.note(&self.ctx.input, event);
            }
        }
        if let Some(probe) = self.key_probe.as_mut() {
            probe.end_frame(&self.ctx.input);
        }
        self.tick(dt);
        self.sync_text_field();
        self.render();
    }

    /// Tell the shell whether the active screen takes typed text.
    fn sync_text_field(&mut self) {
        let Some(shell) = self.shell.as_mut() else {
            return;
        };
        let open = self.ctx.state().is_some_and(|state| {
            state
                .try_borrow()
                .is_ok_and(|state| state.captures_text_input())
        });
        shell.set_text_field(open);
    }

    /// Main loop. `max_frames` runs that many frames then exits cleanly;
    /// used by the `--smoke` build check.
    pub fn run(&mut self, max_frames: Option<u32>) {
        self.run_with_player_input(max_frames, |_, _| true);
    }

    /// Run the normal windowed loop while a bounded policy stages player
    /// inputs for its next frame. The policy receives only
    /// [`PlayerInputFrame`], so it cannot bypass input dispatch or drive
    /// physics.
    pub fn run_with_player_input<F>(&mut self, max_frames: Option<u32>, mut policy: F)
    where
        F: FnMut(&mut PlayerInputFrame<'_>, f64) -> bool,
    {
        self.ctx.running = true;
        let factory = self
            .initial_state
            .take()
            .unwrap_or_else(|| Box::new(placeholder_main_menu));
        let first = factory(&mut self.ctx);
        self.push_shared(first);
        boot_timing::mark("first screen");
        self.ctx.services.presence.start(); // after init; never blocks if Discord is absent
        self.ctx.services.online.start(); // opt-in drivers board; dormant unless confirmed
        self.ctx.services.duty.start(); // opt-in on/off duty notices; dormant unless on
        self.ctx.services.cloud.start(); // opt-in save backup; dormant unless confirmed
        boot_timing::mark("background services started");
        let mut frames = 0u32;
        while self.ctx.running {
            let dt = self.clock.tick(FPS);
            let keep_running = {
                let mut input = PlayerInputFrame { app: self };
                policy(&mut input, dt)
            };
            if !keep_running {
                self.ctx.running = false;
                break;
            }
            let dt = if std::mem::take(&mut self.world_held) {
                0.0
            } else {
                dt
            };
            self.frame(dt);
            frames += 1;
            if frames == 1 {
                boot_timing::mark("first frame");
            }
            if max_frames.is_some_and(|max| frames >= max) {
                self.ctx.running = false;
            }
        }
        self.shutdown();
    }

    pub fn shutdown(&mut self) {
        // Close the phase clock on the session first, or the save below is
        // charged with every millisecond since launch and the one quit phase
        // a player actually waits on reads as hours (Jessie's log, 2026-09-01:
        // "quit: saved 13292556 ms" for a save that took 700).
        boot_timing::mark("quit: requested");
        // No career save here. Every exit from the terminal (Escape, "Quit to
        // main menu", the close button's confirmation) saves on its way out,
        // a drive can only be saved at a stop, and the title has nothing new.
        // The quit-time save only rewrote a matching file -- and each rewrite
        // queued a cloud backup that quit then waited up to five seconds on
        // (Jessie's log, 2026-09-01). Only the settings are written here.
        if let Err(e) = self.ctx.settings.save() {
            log::warn!("Could not save settings: {e}");
        }
        boot_timing::mark("quit: saved");
        // Pump SDL events through every bounded wait below: the window
        // stays up through quit so the screen reader keeps focus for
        // "Installing the update...", and macOS can report an unpumped
        // window as not responding (issue 266). Captures only `self.shell`, so the services borrow
        // freely beside it.
        let shell = &mut self.shell;
        let mut pumps = 0u32;
        let mut longest_gap = Duration::ZERO;
        let mut last_pump = Instant::now();
        let mut pump = || {
            let now = Instant::now();
            longest_gap = longest_gap.max(now - last_pump);
            last_pump = now;
            pumps += 1;
            if let Some(shell) = shell.as_mut() {
                shell.pump_during_quit();
            }
        };
        let presence = self.ctx.services.presence.clone();
        let online = self.ctx.services.online.clone();
        let duty = self.ctx.services.duty.clone();
        let cloud = self.ctx.services.cloud.clone();
        run_while_pumping(
            move || {
                let started = Instant::now();
                presence.shutdown();
                log::info!(
                    "quit: rich presence in {} ms",
                    started.elapsed().as_millis()
                );
                let started = Instant::now();
                online.shutdown();
                log::info!(
                    "quit: drivers board in {} ms",
                    started.elapsed().as_millis()
                );
                let started = Instant::now();
                duty.shutdown();
                log::info!("quit: duty watch in {} ms", started.elapsed().as_millis());
                let started = Instant::now();
                cloud.shutdown(); // flushes the final save's backup, bounded
                log::info!("quit: cloud backup in {} ms", started.elapsed().as_millis());
            },
            &mut pump,
        );
        boot_timing::mark("quit: online services");
        self.ctx
            .synth_worker
            .shutdown_pumping(Duration::from_millis(2500), &mut pump);
        boot_timing::mark("quit: synthesized music");
        profile_module::set_save_listener(None);
        self.ctx.controller.shutdown();
        boot_timing::mark("quit: controller");
        self.ctx.audio.shutdown();
        boot_timing::mark("quit: audio");
        self.ctx.speech.shutdown_pumping(&mut pump);
        boot_timing::mark("quit: speech");
        if let Some(shell) = self.shell.take() {
            shell.shutdown_for_process_exit();
        }
        boot_timing::mark("quit: window");
        log::info!(
            "quit: pumped events {pumps} times, longest gap {} ms",
            longest_gap.as_millis()
        );
    }
}

/// Run `work` off the calling thread while `pump` runs on it until the
/// work finishes: each service shutdown keeps its own bound, but the
/// window keeps answering the OS through the total (issue 266). Runs
/// `work` inline when the helper thread cannot be spawned.
fn run_while_pumping(work: impl FnOnce() + Send + 'static, pump: &mut dyn FnMut()) {
    // `Builder::spawn` consumes the closure even when it fails, so hold it
    // in a slot the spawned thread takes; a failed spawn leaves it for
    // the inline fallback.
    let slot = Arc::new(Mutex::new(Some(work)));
    let in_thread = Arc::clone(&slot);
    let handle = std::thread::Builder::new()
        .name("quit-services".into())
        .spawn(move || {
            if let Some(work) = in_thread.lock().unwrap_or_else(|e| e.into_inner()).take() {
                work();
            }
        });
    match handle {
        Ok(handle) => {
            while !handle.is_finished() {
                pump();
                std::thread::sleep(Duration::from_millis(10));
            }
            if handle.join().is_err() {
                log::warn!("quit: the services thread panicked");
            }
        }
        Err(e) => {
            log::warn!("quit: the services thread did not start ({e}); shutting down inline");
            if let Some(work) = slot.lock().unwrap_or_else(|e| e.into_inner()).take() {
                work();
            }
        }
    }
}

/// The `--smoke` build check: prove world data loads, sound assets are
/// readable, and the deepest load path (continuing a career) finds every
/// baked runtime data file -- a missing file must fail the build here, not
/// a player's first Continue career.
pub fn smoke_checks() -> Result<(), String> {
    let _ = get_world();
    crate::audio::verify_sound_assets().map_err(|e| format!("smoke: {e}"))?;
    if ff_core::data::buffs::buff_catalog().is_empty() {
        return Err("smoke: buff catalog is empty".to_string());
    }
    if ff_core::data::curves::leg_curves("aberdeen_sd_us:pierre_sd_us", true).is_empty() {
        return Err("smoke: curve shard is empty".to_string());
    }
    let approaches = ff_core::data::world_local_data::load_facility_approaches(
        &ff_core::data::data_resources::data_path("facility_approaches.json"),
    )
    .map_err(|e| format!("smoke: {e}"))?;
    if approaches.is_empty() {
        return Err("smoke: facility approaches are empty".to_string());
    }
    ff_core::radio::load_radio_catalog(ff_core::data::data_resources::data_root())
        .map_err(|e| format!("smoke: {e}"))?;
    // And that the online driver token can still reach the platform secret
    // store, which a packaged build can lose silently.
    let (store_ok, store_detail) = crate::online_presence::secret_store_report();
    log::info!("Secret store: {store_detail}");
    if !store_ok {
        return Err(format!(
            "Secret store unreachable in this build: {store_detail}"
        ));
    }
    Ok(())
}

/// Prove a packaged headless build can dynamically load and initialise BASS.
///
/// BASS is opened through `libloading`, not Mach-O's static dependency table,
/// so merely starting the executable does not validate the library beside it.
/// The no-sound device exercises the real loader and required exports without
/// needing an interactive audio session on a hosted runner.
pub fn smoke_audio_checks() -> Result<(), String> {
    let backend = BassBackend::new_headless()
        .map_err(|e| format!("smoke: packaged BASS runtime could not initialize: {e}"))?;
    let mut audio = AudioEngine::with_backend(Box::new(backend));
    audio.shutdown();
    Ok(())
}

/// Command-line switches `main` understands.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CliOptions {
    /// CI: boot, render a few frames, exit 0.
    pub smoke: bool,
    /// No window: the headless app with the null speech sink.
    pub headless: bool,
    /// The controller diagnostics tool instead of the game.
    pub controller_diagnostics: bool,
}

impl CliOptions {
    pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Self {
        let mut options = Self::default();
        for arg in args {
            match arg.as_str() {
                "--smoke" => options.smoke = true,
                "--headless" => options.headless = true,
                "--controller-diagnostics" => options.controller_diagnostics = true,
                _ => {}
            }
        }
        options
    }
}

/// `freight_fate.app.main()`: the process entry, returning the exit code.
pub fn main_with(options: CliOptions) -> i32 {
    configure_logging();
    boot_timing::mark("start up");
    if options.controller_diagnostics {
        return crate::controller::diagnostics::run_controller_diagnostics();
    }
    let mut guard = crate::single_instance::SingleInstanceGuard::new();
    if !guard.acquire() {
        log::warn!("Freight Fate is already running.");
        return 0;
    }
    boot_timing::mark("single instance check");
    let code = run_game(&options);
    guard.release();
    boot_timing::mark("quit: done");
    code
}

fn run_game(options: &CliOptions) -> i32 {
    let max_frames = options.smoke.then_some(5);
    if options.smoke {
        if let Err(e) = smoke_checks() {
            log::error!("Fatal error: {e}");
            return 1;
        }
        if options.headless {
            if let Err(e) = smoke_audio_checks() {
                log::error!("Fatal error: {e}");
                return 1;
            }
        }
        boot_timing::mark("smoke checks");
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut app = if options.headless {
            App::new_headless(Box::new(NullSpeech))
        } else {
            match App::new() {
                Ok(app) => app,
                Err(e) => {
                    log::error!("Fatal error: {e}");
                    return 1;
                }
            }
        };
        app.run(max_frames);
        0
    }));
    match result {
        Ok(code) => code,
        Err(_) => {
            log::error!("Fatal error");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{run_while_pumping, staged_road_handoff};
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    #[test]
    fn run_while_pumping_pumps_until_the_work_finishes() {
        // The work finishes only after it has seen three pumps, so the test
        // pins "keeps pumping while the work runs" without betting on how
        // many 10 ms sleeps a busy CI runner fits into a fixed window. The
        // deadline only stops a broken pump loop from hanging the run.
        let pumps = Arc::new(AtomicU32::new(0));
        let seen = Arc::clone(&pumps);
        let saw_pumps = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&saw_pumps);
        run_while_pumping(
            move || {
                let deadline = Instant::now() + Duration::from_secs(10);
                while seen.load(Ordering::SeqCst) < 3 && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(1));
                }
                flag.store(seen.load(Ordering::SeqCst) >= 3, Ordering::SeqCst);
            },
            &mut || {
                pumps.fetch_add(1, Ordering::SeqCst);
            },
        );
        assert!(
            saw_pumps.load(Ordering::SeqCst),
            "the work finished without the loop pumping, got {} pumps",
            pumps.load(Ordering::SeqCst)
        );
    }

    #[test]
    fn staged_handoff_does_not_invent_truck_state() {
        let text = staged_road_handoff("Seattle departure", 0.0);

        assert_eq!(
            text,
            "Staged: Seattle departure. You take the wheel 0.0 miles up the road."
        );
    }
}

/// The operating system's key events removed; everything else (focus, quit,
/// controller) kept in order.
pub fn without_operator_keys(events: Vec<InputEvent>) -> Vec<InputEvent> {
    events
        .into_iter()
        .filter(|event| !matches!(event, InputEvent::KeyDown { .. } | InputEvent::KeyUp { .. }))
        .collect()
}

#[cfg(test)]
mod operator_keys_tests {
    use super::without_operator_keys;
    use crate::states::base::{InputEvent, Key, Mods};

    #[test]
    fn operator_keys_are_dropped_and_the_rest_kept_in_order() {
        let events = vec![
            InputEvent::WindowFocusGained,
            InputEvent::KeyDown {
                key: Key::Space,
                mods: Mods::NONE,
                text: Some(' '),
                repeat: false,
            },
            InputEvent::KeyUp {
                key: Key::Space,
                mods: Mods::NONE,
            },
            InputEvent::Quit,
        ];
        let kept = without_operator_keys(events);
        assert!(matches!(
            kept.as_slice(),
            [InputEvent::WindowFocusGained, InputEvent::Quit]
        ));
    }
}
