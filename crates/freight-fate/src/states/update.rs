//! Update screens: check, prompt, what's-new reader, and download (port of
//! `freight_fate/states/update.py`).
//!
//! All fully spoken, matching the rest of the game's menus. The check and
//! the download run on background threads; the states poll them every frame
//! and speak progress, so the game loop (and the screen reader) never blocks
//! on the network.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use ff_core::pyfmt::fmt_f;
use ff_core::settings::Settings;

use crate::app::{version, GameContext, Say};
use crate::impl_state_for_menu;
use crate::net;
use crate::states::base::{InputEvent, Key, Menu, MenuCore, MenuItem, State};
use crate::updater::{self, DownloadError, UpdateInfo, UpdaterEnv};

/// Background release check; poll [`UpdateChecker::is_done`] from the main
/// loop.
#[derive(Clone)]
pub struct UpdateChecker {
    done: Arc<AtomicBool>,
    result: Arc<Mutex<Option<UpdateInfo>>>,
    error: Arc<Mutex<Option<String>>>,
}

impl UpdateChecker {
    /// Start the check on a daemon thread.
    pub fn new(settings: &Settings) -> Self {
        let checker = Self::idle();
        let build = updater::load_build_info(version());
        let channel = updater::resolve_channel(&settings.update_channel, build.as_ref());
        let worker = checker.clone();
        let current = version().to_string();
        std::thread::Builder::new()
            .name("update-check".into())
            .spawn(move || worker.run(&channel, &current, build.as_ref()))
            .ok();
        checker
    }

    /// A checker that has already finished with the given answer (the test
    /// seam the Python suite reached with a `SimpleNamespace(done, result)`).
    pub fn finished(result: Option<UpdateInfo>, error: Option<String>) -> Self {
        let checker = Self::idle();
        *checker.result.lock().unwrap_or_else(|e| e.into_inner()) = result;
        *checker.error.lock().unwrap_or_else(|e| e.into_inner()) = error;
        checker.done.store(true, Ordering::SeqCst);
        checker
    }

    /// A checker nothing has started: never done (`threading.Event()` unset).
    pub fn idle() -> Self {
        Self {
            done: Arc::new(AtomicBool::new(false)),
            result: Arc::new(Mutex::new(None)),
            error: Arc::new(Mutex::new(None)),
        }
    }

    fn run(&self, channel: &str, current: &str, build: Option<&updater::BuildInfo>) {
        match updater::check_for_update(channel, current, build) {
            Ok(result) => {
                *self.result.lock().unwrap_or_else(|e| e.into_inner()) = result;
            }
            Err(e) => {
                // offline, rate-limited, GitHub down...
                log::warn!("Update check failed: {e:?}");
                *self.error.lock().unwrap_or_else(|e| e.into_inner()) = Some(format!(
                    "Could not reach the update server. {}",
                    net::describe_error(&e)
                ));
            }
        }
        self.done.store(true, Ordering::SeqCst);
    }

    /// `checker.done.is_set()`.
    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }

    /// `checker.result`.
    pub fn result(&self) -> Option<UpdateInfo> {
        self.result
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// `checker.error`.
    pub fn error(&self) -> Option<String> {
        self.error.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// Manual 'Check for updates' from the Settings menu.
pub struct UpdateCheckState {
    pub checker: Option<UpdateChecker>,
    pub message: String,
}

impl UpdateCheckState {
    pub fn new() -> Self {
        Self {
            checker: None,
            message: String::new(),
        }
    }
}

impl Default for UpdateCheckState {
    fn default() -> Self {
        Self::new()
    }
}

impl State for UpdateCheckState {
    fn enter(&mut self, ctx: &mut GameContext) {
        if !updater::SELF_UPDATES {
            self.message = "On iPhone and iPad, updates come through the App Store \
                            or TestFlight."
                .to_string();
            ctx.say(&format!("{} Escape goes back.", self.message));
            return;
        }
        if !updater::is_frozen() {
            self.message = "Updates are only available in the packaged game. \
                            This copy runs from source; update it with git."
                .to_string();
            ctx.say(&format!("{} Escape goes back.", self.message));
            return;
        }
        if self.checker.is_none() {
            ctx.say("Checking for updates...");
            self.checker = Some(UpdateChecker::new(&ctx.settings));
        }
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
        let Some(c) = &self.checker else {
            return;
        };
        if !c.is_done() || !self.message.is_empty() {
            return;
        }
        if let Some(error) = c.error() {
            self.message = format!("{error} Try again later.");
        } else if let Some(info) = c.result() {
            ctx.replace_state(UpdatePromptState::new(info));
            return;
        } else {
            self.message = format!(
                "You are up to date. Freight Fate version {}.",
                updater::spoken_version(version())
            );
        }
        ctx.say(&format!("{} Escape goes back.", self.message));
    }

    fn handle_event(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        let Some((key, _, _)) = event.key_down() else {
            return;
        };
        if matches!(key, Key::Escape | Key::Return | Key::KpEnter) {
            ctx.audio.play("ui/menu_back");
            ctx.pop_state();
        }
    }

    fn lines(&self, _ctx: &GameContext) -> Vec<String> {
        let status = if self.message.is_empty() {
            "Checking for updates...".to_string()
        } else {
            self.message.clone()
        };
        vec!["Check for updates".to_string(), String::new(), status]
    }
}

/// Asks whether to download a newly found update.
pub struct UpdatePromptState {
    menu: MenuCore<Self>,
    pub info: UpdateInfo,
}

impl UpdatePromptState {
    pub fn new(info: UpdateInfo) -> Self {
        Self {
            menu: MenuCore::new("Update available").with_intro_help(
                "Download and restart installs it now. What's new reads the \
                 changes. Skip this version stops asking about this update.",
            ),
            info,
        }
    }

    fn download(&mut self, ctx: &mut GameContext) {
        if !updater::is_frozen() {
            ctx.say(
                "Updates can only be installed in the packaged game. This copy runs from source.",
            );
            return;
        }
        ctx.replace_state(UpdateDownloadState::new(self.info.clone()));
    }

    fn whats_new(&mut self, ctx: &mut GameContext) {
        ctx.push_state(WhatsNewState::new(self.info.clone()));
    }

    fn skip(&mut self, ctx: &mut GameContext) {
        ctx.settings.skipped_update = self.info.tag.clone();
        if let Err(e) = ctx.settings.save() {
            log::warn!("Could not save settings: {e}");
        }
        ctx.say(&format!(
            "Skipping {}. The next update will still be offered.",
            self.info.title
        ));
        ctx.pop_state();
    }
}

/// `asset_size / 1e6` spoken as whole megabytes, or `None` when unknown.
fn megabytes(info: &UpdateInfo) -> Option<String> {
    let mb = info.asset_size as f64 / 1e6;
    (mb != 0.0).then(|| fmt_f(mb, 0))
}

impl Menu for UpdatePromptState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let size = megabytes(&self.info)
            .map(|mb| format!(" {mb} megabytes."))
            .unwrap_or_default();
        let text = format!(
            "Update available. {} is ready to install. You are on version {}.{size} {}",
            self.info.title,
            updater::spoken_version(version()),
            self.current_text(ctx)
        );
        ctx.say(&text);
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![
            MenuItem::new("Download and restart", |s: &mut Self, ctx| s.download(ctx))
                .help("Download, then restart into the new version."),
            MenuItem::new("What's new", |s: &mut Self, ctx| s.whats_new(ctx))
                .help("The changes in this update, line by line."),
            MenuItem::new("Remind me later", |s: &mut Self, ctx| s.go_back(ctx)).help(
                "Ask again on returning to the main menu from a terminal or pickup facility, or the next time the game starts.",
            ),
            MenuItem::new("Skip this version", |s: &mut Self, ctx| s.skip(ctx))
                .help("Never ask about this update again. Later updates are still offered."),
        ]
    }
}

impl_state_for_menu!(UpdatePromptState);

/// Line-by-line reader for the update's release notes.
pub struct WhatsNewState {
    pub info: UpdateInfo,
    pub notes: Vec<String>,
    pub line: i64,
}

impl WhatsNewState {
    pub fn new(info: UpdateInfo) -> Self {
        let notes = if info.notes.is_empty() {
            vec!["No change notes were provided.".to_string()]
        } else {
            info.notes.clone()
        };
        Self {
            info,
            notes,
            line: -1,
        }
    }
}

impl State for WhatsNewState {
    fn enter(&mut self, ctx: &mut GameContext) {
        ctx.say(&format!(
            "What's new in {}. {} lines. Up and Down arrows read line \
             by line, Enter reads everything, Escape goes back.",
            self.info.title,
            self.notes.len()
        ));
    }

    fn handle_event(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        let Some((key, _, _)) = event.key_down() else {
            return;
        };
        match key {
            Key::Escape => {
                ctx.audio.play("ui/menu_back");
                ctx.pop_state();
            }
            Key::Down => {
                self.line = (self.line + 1).min(self.notes.len() as i64 - 1);
                ctx.say(&self.notes[self.line as usize]);
            }
            Key::Up => {
                self.line = (self.line - 1).max(0);
                ctx.say(&self.notes[self.line as usize]);
            }
            Key::Return | Key::KpEnter | Key::Space => {
                ctx.say(&self.notes.join(" "));
            }
            _ => {}
        }
    }

    fn lines(&self, _ctx: &GameContext) -> Vec<String> {
        let mut out = vec![format!("What's new - {}", self.info.title), String::new()];
        for (i, text) in self.notes.iter().take(14).enumerate() {
            let marker = if i as i64 == self.line { "> " } else { "  " };
            out.push(format!("{marker}{text}"));
        }
        out
    }
}

/// The worker's shared outcome: written by the download thread, read by
/// `update` on the main thread.
#[derive(Default)]
struct DownloadOutcome {
    new_root: Option<PathBuf>,
    staging: Option<PathBuf>,
    error: Option<String>,
}

/// `updater.can_auto_apply` / `stash_for_manual_install`, injectable so a
/// test can stage an update that needs a manual install without a real
/// AppImage (the Python tests monkeypatched the module functions).
pub type AutoApplyProbe = Box<dyn Fn(&Path) -> bool>;
pub type StashHook = Box<dyn Fn(&Path) -> PathBuf>;
/// `updater::apply_and_restart`, injectable so a test can make the apply
/// script fail to start without spawning a real shell.
pub type ApplyHook = Box<dyn Fn(&Path, &Path) -> std::io::Result<()>>;
/// `updater::download`, injectable so a test can play a transfer that
/// stalls or fails part-way without the network.
pub type FetchHook = Box<
    dyn FnOnce(
            &UpdateInfo,
            &Path,
            &mut dyn FnMut(u64, u64),
            &AtomicBool,
        ) -> Result<PathBuf, DownloadError>
        + Send,
>;

/// Seconds without a byte before this screen gives up on the worker.
///
/// The worker fails its own transfer after
/// [`updater::DOWNLOAD_IDLE_TIMEOUT`] and says why; this is the backstop
/// for a worker that cannot even report, so the screen can never hold the
/// player (issue 266).
pub const STALL_TIMEOUT_S: f64 = updater::DOWNLOAD_IDLE_TIMEOUT.as_secs_f64() + 30.0;
/// Seconds the unpack may take on this screen's clock: the worker's own
/// unpacker bound, plus a moment for it to report back.
pub const UNPACK_TIMEOUT_S: f64 = updater::UNPACK_TIMEOUT.as_secs_f64() + 30.0;

/// `text` as a spoken sentence: capitalized, with a closing full stop.
fn sentence(text: &str) -> String {
    let text = text.trim();
    let mut chars = text.chars();
    let mut out = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
        None => return String::new(),
    };
    if !out.ends_with(['.', '!', '?']) {
        out.push('.');
    }
    out
}

/// Where a player can always get the update by hand.
pub const MANUAL_DOWNLOAD: &str =
    "To update by hand, download it from github.com/orinks-games/Freight-Fate/releases.";

const PHASE_DOWNLOADING: u8 = 0;
const PHASE_UNPACKING: u8 = 1;

/// Downloads and stages the update, then restarts the game.
///
/// Everything slow -- the transfer and the unpack -- runs on the worker
/// thread. This screen only polls it, so it can always speak and always be
/// left: Escape leaves at once, a transfer that goes quiet for
/// [`STALL_TIMEOUT_S`] or an unpack past [`UNPACK_TIMEOUT_S`] is abandoned
/// with a spoken reason and the manual route, and the game is unchanged.
pub struct UpdateDownloadState {
    pub info: UpdateInfo,
    cancelled: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
    outcome: Arc<Mutex<DownloadOutcome>>,
    /// 0..1, written by the worker thread (as f64 bits).
    progress: Arc<AtomicU64>,
    /// Bytes received so far, written by the worker thread.
    bytes: Arc<AtomicU64>,
    /// [`PHASE_DOWNLOADING`] or [`PHASE_UNPACKING`], written by the worker.
    phase: Arc<AtomicU8>,
    spoken_quarter: i64,
    finished: bool,
    started: bool,
    /// The byte count last seen, and seconds since it last moved.
    seen_bytes: u64,
    idle_s: f64,
    unpack_s: f64,
    unpack_spoken: bool,
    can_auto_apply: AutoApplyProbe,
    stash_for_manual_install: StashHook,
    apply: ApplyHook,
    fetch: Option<FetchHook>,
}

impl UpdateDownloadState {
    pub fn new(info: UpdateInfo) -> Self {
        Self {
            info,
            cancelled: Arc::new(AtomicBool::new(false)),
            done: Arc::new(AtomicBool::new(false)),
            outcome: Arc::new(Mutex::new(DownloadOutcome::default())),
            progress: Arc::new(AtomicU64::new(0f64.to_bits())),
            bytes: Arc::new(AtomicU64::new(0)),
            phase: Arc::new(AtomicU8::new(PHASE_DOWNLOADING)),
            spoken_quarter: 0,
            finished: false,
            started: false,
            seen_bytes: 0,
            idle_s: 0.0,
            unpack_s: 0.0,
            unpack_spoken: false,
            can_auto_apply: Box::new(|root| updater::can_auto_apply(root, &UpdaterEnv::current())),
            // The home folder, so a parked update is somewhere the player
            // can find again, not under the system temp folder.
            stash_for_manual_install: Box::new(|root| {
                updater::stash_for_manual_install(root, UpdaterEnv::current().home.as_deref())
            }),
            apply: Box::new(updater::apply_and_restart),
            fetch: Some(Box::new(|info, dir, progress, cancelled| {
                updater::download(info, dir, Some(progress), Some(cancelled))
            })),
        }
    }

    /// Test seam: a download that has already finished with `new_root`
    /// staged under `staging` (`state.staging = ...; state.new_root = ...;
    /// state.done.set()` in the Python tests), with the apply probes
    /// injected.
    pub fn finished_with(
        info: UpdateInfo,
        staging: PathBuf,
        new_root: PathBuf,
        can_auto_apply: AutoApplyProbe,
        stash_for_manual_install: StashHook,
    ) -> Self {
        let mut state = Self::new(info);
        {
            let mut outcome = state.outcome.lock().unwrap_or_else(|e| e.into_inner());
            outcome.staging = Some(staging);
            outcome.new_root = Some(new_root);
        }
        state.done.store(true, Ordering::SeqCst);
        state.started = true;
        state.can_auto_apply = can_auto_apply;
        state.stash_for_manual_install = stash_for_manual_install;
        state
    }

    /// Test seam: a download already under way whose worker the test plays
    /// by hand through [`report_progress`](Self::report_progress) and
    /// [`report_unpacking`](Self::report_unpacking). Nothing is fetched; a
    /// test that reports nothing is a transfer that has gone quiet.
    pub fn in_progress(info: UpdateInfo) -> Self {
        let mut state = Self::new(info);
        state.started = true;
        state
    }

    /// Replace the transfer (test seam; see [`FetchHook`]).
    pub fn with_fetch(mut self, fetch: FetchHook) -> Self {
        self.fetch = Some(fetch);
        self
    }

    /// The worker's staging folder, once it has made one.
    pub fn staging(&self) -> Option<PathBuf> {
        self.outcome
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .staging
            .clone()
    }

    /// Replace the apply step (test seam; see [`ApplyHook`]).
    pub fn with_apply(mut self, apply: ApplyHook) -> Self {
        self.apply = apply;
        self
    }

    /// What the worker reports as bytes arrive.
    pub fn report_progress(&self, done: u64, total: u64) {
        Self::record_progress(&self.bytes, &self.progress, done, total);
    }

    /// What the worker reports once the archive is on disk.
    pub fn report_unpacking(&self) {
        self.phase.store(PHASE_UNPACKING, Ordering::SeqCst);
    }

    fn record_progress(bytes: &AtomicU64, progress: &AtomicU64, done: u64, total: u64) {
        bytes.store(done, Ordering::SeqCst);
        if total > 0 {
            progress.store((done as f64 / total as f64).to_bits(), Ordering::Relaxed);
        }
    }

    pub fn progress(&self) -> f64 {
        f64::from_bits(self.progress.load(Ordering::Relaxed))
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn error(&self) -> Option<String> {
        self.outcome
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .error
            .clone()
    }

    pub fn new_root(&self) -> Option<PathBuf> {
        self.outcome
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .new_root
            .clone()
    }

    #[allow(clippy::too_many_arguments)]
    fn work(
        fetch: FetchHook,
        info: UpdateInfo,
        outcome: Arc<Mutex<DownloadOutcome>>,
        progress: Arc<AtomicU64>,
        bytes: Arc<AtomicU64>,
        phase: Arc<AtomicU8>,
        cancelled: Arc<AtomicBool>,
        done: Arc<AtomicBool>,
    ) {
        let is_cancelled = || cancelled.load(Ordering::SeqCst);
        let mut staged_in: Option<PathBuf> = None;
        let result = (|| -> Result<(), DownloadError> {
            let staging = updater::make_staging_dir()?;
            staged_in = Some(staging.clone());
            outcome.lock().unwrap_or_else(|e| e.into_inner()).staging = Some(staging.clone());
            let mut on_progress =
                |done: u64, total: u64| Self::record_progress(&bytes, &progress, done, total);
            let archive = fetch(&info, &staging, &mut on_progress, &cancelled)?;
            if is_cancelled() {
                return Err(DownloadError::Cancelled);
            }
            phase.store(PHASE_UNPACKING, Ordering::SeqCst);
            log::info!("Update downloaded; unpacking {}", archive.display());
            let new_root = updater::stage_update_with(
                &archive,
                &staging,
                &UpdaterEnv::current(),
                Some(&cancelled),
                updater::UNPACK_TIMEOUT,
            )?;
            if is_cancelled() {
                return Err(DownloadError::Cancelled);
            }
            log::info!("Update unpacked to {}", new_root.display());
            outcome.lock().unwrap_or_else(|e| e.into_inner()).new_root = Some(new_root);
            Ok(())
        })();
        if result.is_err() {
            // Nothing will use a half-finished download: never leave up to
            // 420 MB of it in the temp folder.
            if let Some(staging) = &staged_in {
                let _ = std::fs::remove_dir_all(staging);
            }
        }
        let message = match result {
            Ok(()) => None,
            // The screen already left (Escape, or it gave up on a stall):
            // nothing will read this outcome.
            Err(_) if is_cancelled() => {
                log::info!("Update download stopped after the screen left it");
                None
            }
            Err(DownloadError::Cancelled) => None,
            Err(DownloadError::Stalled(idle)) => Some(format!(
                "The download stopped. Nothing arrived for {} seconds.",
                idle.as_secs()
            )),
            Err(DownloadError::Corrupt) => {
                Some("The download arrived damaged, so it was not installed.".to_string())
            }
            Err(DownloadError::Net(e)) => {
                log::warn!("Update download failed: {e:?}");
                Some(format!("The download failed. {}", net::describe_error(&e)))
            }
            Err(DownloadError::Io(e)) => {
                log::warn!("Update download failed: {e:?}");
                Some(format!("The download failed. {}", sentence(&e.to_string())))
            }
        };
        if let Some(message) = message {
            outcome.lock().unwrap_or_else(|e| e.into_inner()).error = Some(format!(
                "{message} Your game is unchanged. Try again later. {MANUAL_DOWNLOAD}"
            ));
        }
        done.store(true, Ordering::SeqCst);
    }

    /// Leave the screen on a stalled or wedged worker, telling the player
    /// why and how to get the update anyway. The worker is told to stop;
    /// it tidies up whenever it next wakes.
    fn abandon(&mut self, ctx: &mut GameContext, reason: &str) {
        log::warn!("Update abandoned: {reason}");
        self.cancelled.store(true, Ordering::SeqCst);
        self.finished = true;
        ctx.say(&format!(
            "{reason} Your game is unchanged. Try again later. {MANUAL_DOWNLOAD}"
        ));
        ctx.audio.play("ui/error");
        ctx.pop_state();
    }

    /// The stall and unpack watchdogs, on this screen's own clock.
    fn watch(&mut self, ctx: &mut GameContext, dt: f64) {
        if self.phase.load(Ordering::SeqCst) == PHASE_UNPACKING {
            if !self.unpack_spoken {
                self.unpack_spoken = true;
                ctx.say_with(
                    "Download complete. Unpacking the update.".to_string(),
                    Say::queued(),
                );
            }
            self.unpack_s += dt;
            if self.unpack_s >= UNPACK_TIMEOUT_S {
                self.abandon(ctx, "Unpacking the update took too long.");
            }
            return;
        }
        let bytes = self.bytes.load(Ordering::SeqCst);
        if bytes != self.seen_bytes {
            self.seen_bytes = bytes;
            self.idle_s = 0.0;
            return;
        }
        self.idle_s += dt;
        if self.idle_s >= STALL_TIMEOUT_S {
            self.abandon(
                ctx,
                &format!(
                    "The download stopped. Nothing arrived for {} seconds.",
                    STALL_TIMEOUT_S as i64
                ),
            );
        }
    }
}

impl State for UpdateDownloadState {
    fn enter(&mut self, ctx: &mut GameContext) {
        if self.started {
            return;
        }
        let size = megabytes(&self.info)
            .map(|mb| format!(", {mb} megabytes"))
            .unwrap_or_default();
        ctx.say(&format!(
            "Downloading {}{size}. The game restarts when the download \
             finishes. Escape cancels.",
            self.info.title
        ));
        self.started = true;
        let info = self.info.clone();
        let outcome = Arc::clone(&self.outcome);
        let progress = Arc::clone(&self.progress);
        let bytes = Arc::clone(&self.bytes);
        let phase = Arc::clone(&self.phase);
        let cancelled = Arc::clone(&self.cancelled);
        let done = Arc::clone(&self.done);
        let Some(fetch) = self.fetch.take() else {
            return;
        };
        let spawned = std::thread::Builder::new()
            .name("update-download".into())
            .spawn(move || {
                Self::work(
                    fetch, info, outcome, progress, bytes, phase, cancelled, done,
                )
            });
        if let Err(e) = spawned {
            log::warn!("Could not start the update download: {e}");
            self.abandon(ctx, "The download could not start.");
        }
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
        if self.finished {
            return;
        }
        let quarter = (self.progress() * 4.0) as i64;
        if quarter > self.spoken_quarter && quarter < 4 {
            self.spoken_quarter = quarter;
            ctx.say_with(format!("{} percent.", quarter * 25), Say::queued());
        }
        if !self.is_done() {
            self.watch(ctx, dt);
            return;
        }
        self.finished = true;
        let (new_root, staging, error) = {
            let outcome = self.outcome.lock().unwrap_or_else(|e| e.into_inner());
            (
                outcome.new_root.clone(),
                outcome.staging.clone(),
                outcome.error.clone(),
            )
        };
        if self.is_cancelled() {
            ctx.pop_state();
            return;
        }
        let Some(new_root) = new_root.filter(|_| error.is_none()) else {
            ctx.say(&error.unwrap_or_else(|| "The download failed.".to_string()));
            ctx.audio.play("ui/error");
            ctx.pop_state();
            return;
        };
        if !(self.can_auto_apply)(&new_root) {
            // e.g. an AppImage sitting in a folder this user cannot write
            // to, or a Mac app opened straight from a download: the swap
            // would fail, so park the download somewhere findable and say
            // where instead of dead-ending on restart.
            let dest = (self.stash_for_manual_install)(&new_root);
            ctx.say(&format!(
                "Download complete, but this install cannot update itself. \
                 The new version is saved at {}. Install it yourself, then \
                 restart the game.",
                dest.display()
            ));
            ctx.pop_state();
            return;
        }
        let staging = staging.unwrap_or_else(|| new_root.clone());
        if let Err(e) = (self.apply)(&new_root, &staging) {
            // Quitting now would close the game with nothing to bring it
            // back. Stay, and hand the player the update instead.
            log::warn!("Could not spawn the update apply script: {e}");
            let dest = (self.stash_for_manual_install)(&new_root);
            ctx.say(&format!(
                "Download complete, but the update could not install itself. \
                 The new version is saved at {}. Install it yourself, then \
                 restart the game.",
                dest.display()
            ));
            ctx.audio.play("ui/error");
            ctx.pop_state();
            return;
        }
        ctx.say("Installing the update. The game closes and restarts by itself.");
        ctx.quit();
    }

    fn handle_event(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        let Some((key, _, _)) = event.key_down() else {
            return;
        };
        if self.finished {
            return;
        }
        match key {
            Key::Escape => {
                // Leave now, whatever the worker is doing. Waiting for it
                // to notice meant a transfer blocked on a quiet connection,
                // or a long unpack, kept the player here with Escape
                // answering "cancelled" and nothing else (issue 266). The
                // worker stops and tidies up when it next wakes.
                self.cancelled.store(true, Ordering::SeqCst);
                self.finished = true;
                ctx.say("Update cancelled.");
                ctx.audio.play("ui/menu_back");
                ctx.pop_state();
            }
            Key::Tab => {
                let text = if self.phase.load(Ordering::SeqCst) == PHASE_UNPACKING {
                    "Unpacking the update.".to_string()
                } else {
                    format!("{} percent downloaded.", fmt_f(self.progress() * 100.0, 0))
                };
                ctx.say(&text);
            }
            _ => {}
        }
    }

    fn lines(&self, _ctx: &GameContext) -> Vec<String> {
        let status = if self.phase.load(Ordering::SeqCst) == PHASE_UNPACKING {
            "Unpacking".to_string()
        } else {
            format!("{} percent", fmt_f(self.progress() * 100.0, 0))
        };
        vec![
            format!("Downloading {}", self.info.title),
            String::new(),
            status,
            "Escape cancels, Tab reads progress.".to_string(),
        ]
    }
}
