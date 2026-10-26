//! Menus for the orinks.net drivers board: setup flow and the live list
//! (port of `freight_fate/states/online_states.py`).
//!
//! Setup uses the device-code activation flow (see `online_activation`):
//! the game asks orinks.net for a short activation code, speaks it, and the
//! player enters that code in any browser on any device -- no clipboard paste
//! between the two apps. The game polls in the background and adopts the
//! resulting driver identity once the code is claimed. Nothing is transmitted
//! until a code is claimed, and the spoken disclosure below tells the player
//! exactly what sharing will send.
//!
//! All network calls run on daemon threads; the menu states poll a small
//! result slot ([`Mailbox`]) from `update` so the game loop and speech stay
//! responsive throughout. Every state carries a `threaded` switch so tests
//! run the same worker inline against a fake transport.

use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

pub use ff_core::settings::PROFILE_SHARING_CONSENT_VERSION;

use crate::app::{GameContext, Say};
use crate::impl_state_for_menu;
use crate::net::{wait_seconds, Event, Transport};
use crate::online_activation::{self, Activation, PollResult};
use crate::online_presence::{self, OnlineIdentity};
use crate::states::base::{InputEvent, Label, Menu, MenuCore, MenuItem};

mod board;
mod directory;
mod profile;
mod support;

pub use board::{updated_text, DriversOnlineState};
pub use directory::{
    directory_row_text, last_on_duty_text, last_on_duty_text_at, DriverDirectoryState,
};
pub use profile::{profile_rows, DriverProfileState};
pub use support::{
    identity_store, load_identity, menu_default_enter, menu_default_go_back,
    menu_default_handle_event, online_transport, open_url, run_worker, save_identity,
    set_identity_store_override, set_online_transport_override, set_open_url_override, wall_time,
    Mailbox,
};

/// Polling schedule for [`OnlineSetupState`] (a field rather than module
/// constants so tests can shrink it to make a real background thread finish
/// fast, instead of monkeypatching time globally and risking every other
/// timed test in the same run).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PollSchedule {
    pub interval_first_s: f64,
    pub interval_later_s: f64,
    pub first_phase_s: f64,
    pub still_waiting_after_s: f64,
}

pub const ACTIVATION_POLL_INTERVAL_FIRST: f64 = 3.0;
pub const ACTIVATION_POLL_INTERVAL_LATER: f64 = 8.0;
pub const ACTIVATION_POLL_FIRST_PHASE_SECONDS: f64 = 30.0;
pub const ACTIVATION_STILL_WAITING_AFTER: f64 = 5.0;

impl Default for PollSchedule {
    fn default() -> Self {
        Self {
            interval_first_s: ACTIVATION_POLL_INTERVAL_FIRST,
            interval_later_s: ACTIVATION_POLL_INTERVAL_LATER,
            first_phase_s: ACTIVATION_POLL_FIRST_PHASE_SECONDS,
            still_waiting_after_s: ACTIVATION_STILL_WAITING_AFTER,
        }
    }
}

pub const DISCLOSURE: &str =
    "Connecting an orinks.net account turns Profile sharing on and starts backing your \
careers up to that account, so your driver profile has career statistics on it from \
the first delivery. Either one is a single item away in the Online menu whenever you \
want it off. When Profile sharing is on, orinks.net can \
publicly show your driver name and broad on-duty board activity; eligible profile \
details; official achievements you earn; and automatic road-journal posts \
generated from gameplay. Public updates can also appear in the Freight Fate updates \
feed. Each post also tells orinks.net which game version you are running, used only \
for moderation and troubleshooting and never shown publicly. Freight Fate does not \
publish your real name, full save, coordinates, active cargo details, or precise \
real-world location. Detailed career statistics come only from your public career's \
latest accepted private backup, which you choose on the Cloud backup menu. They \
include lifetime career earnings, the running total your career \
has ever earned; the money you currently have is never published. The backups \
themselves stay private to your account and never appear as public downloads. \
Turning Profile sharing off hides public details but does not turn cloud backup off.";

// -- OnlineSetupState -------------------------------------------------------------------------

/// Where the setup flow is (`self._phase`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupPhase {
    Idle,
    Starting,
    Waiting,
    Sharing,
    Expired,
    Error,
}

/// What the setup worker posts back (`self._outcome`).
#[derive(Debug, Clone, PartialEq)]
pub enum SetupOutcome {
    StartFailed,
    Activation(Activation),
    Ready(PollResult),
    /// `online_presence::set_profile_sharing`'s answer.
    Sharing(&'static str),
    Expired,
    Error,
}

/// Request and track an orinks.net activation code for this computer.
///
/// The menu is deliberately STATIC -- the same five items for the
/// whole flow, with labels that carry the captured state -- because players
/// build positional memory of spoken menus and refresh() preserves indices,
/// not item identity. Only item 1's label carries progress; items 2-5 are
/// fixed text.
///
/// The game has no screen reader review cursor -- a player cannot step
/// through a spoken string character by character the way they can in a
/// browser or an editor -- so items 2 and 3 exist purely to let a player
/// replay the activation code as many times as they need: item 2 spells it
/// phonetically, item 3 puts it on the clipboard (a write, the direction
/// that still works even when the game and the player's browser do not
/// share a clipboard). Both stay available for as long as an activation is
/// outstanding, and both double as the fallback path for when
/// the browser opener does nothing. On success the identity is saved, cloud
/// backup starts, and Profile sharing is turned on -- a connected account
/// whose public profile reads "no career statistics yet" is a connection that
/// did nothing for the player, and those statistics are derived from the
/// backup, so the two only make sense together. Either one is a single item
/// away in the Online menu afterwards.
pub struct OnlineSetupState {
    pub menu: MenuCore<Self>,
    pub activation: Option<Activation>,
    pub phase: SetupPhase,
    /// When the code was announced; the "Still waiting." clock. `None` until
    /// the announcement has been queued.
    pub poll_started: Option<Instant>,
    still_waiting_said: bool,
    /// worker -> update() mailbox
    pub outcome: Mailbox<SetupOutcome>,
    /// A fresh Event per run, replaced in `start_setup`: `exit()` must always
    /// have *something* to set, even if the player never starts setup.
    stop: Arc<Event>,
    /// The poll worker, when one was spawned (tests join it).
    pub worker: Option<JoinHandle<()>>,
    /// Set when this state is pushed straight from the offer's "Set up
    /// now" answer: the player already said yes, so entry starts the
    /// request itself instead of making them choose the first menu item
    /// to confirm a decision they already made. Public so callers (and
    /// tests) can see whether a given push will autostart.
    pub autostart: bool,
    /// Workers on threads (the game) or inline (tests). Inline, the start
    /// request runs to completion and posts its outcome, but the poll loop --
    /// which blocks by design -- is not entered; tests drive [`poll_loop`]
    /// themselves.
    pub threaded: bool,
    pub schedule: PollSchedule,
}

impl OnlineSetupState {
    pub const TITLE: &'static str = "orinks.net account setup";

    /// `OnlineSetupState(ctx)`.
    pub fn new(ctx: &mut GameContext) -> Self {
        Self::with_autostart(ctx, false)
    }

    /// `OnlineSetupState(ctx, autostart=True)`.
    pub fn with_autostart(_ctx: &mut GameContext, autostart: bool) -> Self {
        Self {
            menu: MenuCore::new(Self::TITLE),
            activation: None,
            phase: SetupPhase::Idle,
            poll_started: None,
            still_waiting_said: false,
            outcome: Mailbox::new(),
            stop: Arc::new(Event::new()),
            worker: None,
            autostart,
            threaded: true,
            schedule: PollSchedule::default(),
        }
    }

    // -- static menu ----------------------------------------------------------

    pub fn setup_label(&self) -> String {
        match (self.phase, &self.activation) {
            (SetupPhase::Starting, _) => "Starting setup with orinks.net".to_string(),
            (SetupPhase::Waiting, Some(activation)) => {
                format!("Waiting for code {} to be entered", activation.user_code)
            }
            (SetupPhase::Sharing, _) => "Finishing setup with orinks.net".to_string(),
            (SetupPhase::Expired, _) => "Activation code expired, choose for a new one".to_string(),
            (SetupPhase::Error, _) => "Setup could not continue, choose to start over".to_string(),
            _ => "Set up this computer with orinks.net".to_string(),
        }
    }

    fn speak_disclosure(&mut self, ctx: &mut GameContext) {
        ctx.say(DISCLOSURE);
    }

    // -- starting -------------------------------------------------------------

    pub fn start_setup(&mut self, ctx: &mut GameContext) {
        if self.phase == SetupPhase::Sharing {
            // The code was already accepted; the account is being switched on.
            // A fresh activation request here would throw away a finished
            // setup, so this reports where the flow actually is instead.
            ctx.say("Your account is connected. Still turning Profile sharing on.");
            return;
        }
        if matches!(self.phase, SetupPhase::Starting | SetupPhase::Waiting) {
            // Already under way -- repeat the code rather than burning a
            // second activation request the player did not ask for.
            match &self.activation {
                Some(activation) => {
                    ctx.say(&format!(
                        "Still waiting for the code. Your activation code is {}.",
                        activation.user_code
                    ));
                }
                None => {
                    // Phase "starting": the request is in flight and there is no
                    // code to repeat yet. Returning in silence here would read as
                    // "did that keypress even register" -- this game has no visual
                    // fallback to check against.
                    ctx.say("Still contacting orinks.net for an activation code.");
                }
            }
            return;
        }
        self.phase = SetupPhase::Starting;
        self.activation = None;
        self.still_waiting_said = false;
        self.refresh(ctx, true);
        ctx.say("Contacting orinks.net for an activation code.");
        self.stop = Arc::new(Event::new());
        let stop = Arc::clone(&self.stop);
        let outcome = self.outcome.clone();
        let transport = online_transport();
        let schedule = self.schedule;
        let threaded = self.threaded;
        let data_dir = ff_core::models::profile::data_dir();
        self.worker = run_worker(threaded, "online-activation", move || {
            let activation = online_activation::start_activation(transport.as_ref(), &data_dir);
            if stop.is_set() {
                // player already backed out
                return;
            }
            let Some(activation) = activation else {
                outcome.post(SetupOutcome::StartFailed);
                return;
            };
            outcome.post(SetupOutcome::Activation(activation.clone()));
            if threaded {
                poll_loop(&activation, &stop, &outcome, transport.as_ref(), &schedule);
            }
        });
    }

    fn announce_activation(&mut self, ctx: &mut GameContext, activation: &Activation) {
        let code = &activation.user_code;
        let opened = open_url(&activation.verification_uri_complete);
        if opened {
            ctx.say(&format!(
                "Your activation code is {code}. Your browser is open at {} with the code \
                 filled in. Sign in there.",
                activation.verification_uri
            ));
        } else {
            // The browser opener can also silently do nothing without failing
            // (a remote/streamed session is the common case) -- items 2 and 3
            // are the fallback for that case too, not only this one, but this
            // is the one moment the game knows for certain that opening
            // failed, so it is worth naming them here.
            ctx.say(&format!(
                "The browser could not be opened. Your activation code is {code}. In any \
                 browser, go to {} and enter it. Say my activation code again spells it, Copy \
                 my activation code puts it on the clipboard.",
                activation.verification_uri
            ));
        }
    }

    // -- review affordances -----------------------------------------------------

    pub fn repeat_code(&mut self, ctx: &mut GameContext) {
        let Some(activation) = &self.activation else {
            ctx.say("No activation code yet. Choose Set up this computer with orinks.net first.");
            return;
        };
        ctx.say(&format!(
            "Your activation code, spelled out: {}.",
            online_activation::spell_code(&activation.user_code)
        ));
    }

    pub fn copy_code(&mut self, ctx: &mut GameContext) {
        let Some(activation) = &self.activation else {
            ctx.say("No activation code yet. Choose Set up this computer with orinks.net first.");
            return;
        };
        // Never claim a copy that failed -- the clipboard reports whether
        // the text landed before this ever says "copied".
        let code = activation.user_code.clone();
        if ctx.write_clipboard_text(&code) {
            ctx.say("Activation code copied to the clipboard.");
        } else {
            ctx.say(
                "Could not copy to the clipboard. Say my activation code again spells it \
                 instead.",
            );
        }
    }

    // -- polling result -----------------------------------------------------------

    fn finish_success(&mut self, ctx: &mut GameContext, result: PollResult) {
        self.activation = None;
        self.phase = SetupPhase::Idle;
        self.refresh(ctx, true);
        let identity = OnlineIdentity::new(
            result.driver_id.as_deref().unwrap_or(""),
            result.token.as_deref().unwrap_or(""),
        );
        if save_identity(&identity).is_err() {
            ctx.audio.play("ui/error");
            ctx.say(
                "The code was accepted, but this computer could not save the driver token \
                 securely. Nothing was changed. Check your password store, then try Set up \
                 this computer with orinks.net again.",
            );
            return;
        }
        // Cloud backup needs no server handshake -- the next accepted save
        // uploads itself -- so it is on the moment the account is connected.
        // Profile sharing does need one: orinks.net stays the authority on
        // what is public, so `online_presence` only flips once the server
        // has confirmed it, exactly as ProfileSharingSyncState does.
        ctx.settings.cloud_saves = true;
        ctx.settings.online_presence = false;
        ctx.settings.profile_sharing_consent_version = 0;
        ctx.settings.profile_sharing_pending_off = false;
        if let Err(e) = ctx.settings.save() {
            log::warn!("Could not save settings: {e}");
        }
        ctx.adopt_online_identity(Some(identity.clone()));
        ctx.apply_online_presence();
        ctx.apply_cloud_saves();
        ctx.audio.play("ui/menu_select");
        // The display name is not decoration: it is the only way a player
        // finds out someone else claimed the code they spoke or copied, and
        // that the token just saved belongs to a stranger's driver, not theirs.
        let display = match result.display_name.as_deref() {
            Some(name) if !name.is_empty() => name.to_string(),
            _ => "your driver".to_string(),
        };
        self.phase = SetupPhase::Sharing;
        self.refresh(ctx, true);
        ctx.say(&format!(
            "Connected to orinks.net as {display}. Your careers now back up \
             to that account. Turning Profile sharing on."
        ));
        let outcome = self.outcome.clone();
        let transport = online_transport();
        self.worker = run_worker(self.threaded, "profile-sharing", move || {
            outcome.post(SetupOutcome::Sharing(online_presence::set_profile_sharing(
                &identity,
                true,
                transport.as_ref(),
            )));
        });
    }

    /// Apply the server's answer to the setup-time Profile sharing switch.
    ///
    /// A refusal is not a failed setup: the account is connected and backing
    /// up either way, so this says which half landed and names the one item
    /// that retries the other, rather than sending the player back through a
    /// fresh activation code for something the code already did.
    fn finish_sharing(&mut self, ctx: &mut GameContext, outcome: &str) {
        self.phase = SetupPhase::Idle;
        self.refresh(ctx, true);
        if outcome == "ok" {
            ctx.settings.online_presence = true;
            ctx.settings.profile_sharing_consent_version = PROFILE_SHARING_CONSENT_VERSION;
            if let Err(e) = ctx.settings.save() {
                log::warn!("Could not save settings: {e}");
            }
            ctx.apply_online_presence();
            ctx.say(
                "Profile sharing is on. Your driver profile fills in as you drive. Both this \
                 and cloud backup are single items on the Online menu.",
            );
        } else {
            ctx.say(
                "Your account is connected and backing up, but orinks.net could not turn \
                 Profile sharing on, so your profile stays private. Profile sharing on the \
                 Online menu tries again.",
            );
        }
        ctx.pop_state();
    }
}

/// Poll until claimed, expired, told to stop, or terminally rejected.
///
/// Runs entirely on the worker thread. Every reachable exit posts to
/// `outcome` -- except "keep waiting", which posts nothing: update() times
/// the "still waiting" line off `poll_started`, not off a worker message,
/// so a silent pending poll needs no mailbox entry at all. "retry" (a
/// transient network blip or 5xx -- see `online_activation::poll_activation`)
/// is folded into "keep waiting" too: the whole point of splitting it from
/// "error" is that a dropped connection three seconds from now should not
/// force the player back through a fresh activation code.
pub fn poll_loop(
    activation: &Activation,
    stop: &Event,
    outcome: &Mailbox<SetupOutcome>,
    transport: &dyn Transport,
    schedule: &PollSchedule,
) {
    let first_phase_started = Instant::now();
    let first_phase_s = schedule.first_phase_s.max(0.0);
    while !stop.is_set() {
        if wall_time() >= activation.expires_at {
            outcome.post(SetupOutcome::Expired);
            return;
        }
        let result = online_activation::poll_activation(activation, transport);
        if stop.is_set() {
            // player backed out while the request was in flight
            return;
        }
        match result.status.as_str() {
            "ready" => {
                outcome.post(SetupOutcome::Ready(result));
                return;
            }
            "expired" => {
                outcome.post(SetupOutcome::Expired);
                return;
            }
            "error" => {
                outcome.post(SetupOutcome::Error);
                return;
            }
            _ => {}
        }
        // "pending" and "retry" both fall through here: nothing to post,
        // just wait out the interval and poll again.
        let interval = if first_phase_started.elapsed().as_secs_f64() < first_phase_s {
            schedule.interval_first_s
        } else {
            schedule.interval_later_s
        };
        if wait_seconds(stop, interval) {
            // True means stop was requested during the wait
            return;
        }
    }
}

impl Menu for OnlineSetupState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![
            MenuItem::new(
                Label::dynamic(|s: &Self, _| s.setup_label()),
                |s: &mut Self, ctx| s.start_setup(ctx),
            )
            .help(
                "Asks orinks.net for an activation code, tries to open \
                 your browser with it filled in, and waits for you to sign \
                 in there.",
            ),
            MenuItem::new("Say my activation code again", |s: &mut Self, ctx| {
                s.repeat_code(ctx)
            })
            .help("Spells the activation code letter by letter."),
            MenuItem::new("Copy my activation code", |s: &mut Self, ctx| {
                s.copy_code(ctx)
            })
            .help("Puts the activation code on the clipboard."),
            MenuItem::new("Hear what gets shared", |s: &mut Self, ctx| {
                s.speak_disclosure(ctx)
            }),
            MenuItem::new("Cancel", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Leave without connecting this account."),
        ]
    }

    fn enter(&mut self, ctx: &mut GameContext) {
        menu_default_enter(self, ctx);
        if self.autostart {
            self.start_setup(ctx);
        }
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        if self.autostart {
            // The player already said "Set up now" on the offer -- hearing
            // this five-item menu introduced, only to have start_setup talk
            // over it a moment later with "Contacting orinks.net...", reads
            // as the game losing its place. Say nothing here; start_setup
            // (called right after enter() finishes) speaks first instead.
            return;
        }
        let current = self.current_text(ctx);
        ctx.say(&format!(
            "{}. Connects the game to your orinks.net account, turning Profile sharing on \
             and backing your careers up. Hear what gets shared has the details. {current}",
            Self::TITLE
        ));
    }

    fn handle_event(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        // Re-orient after the browser round trip: this flow is a two-app
        // dance, and "where was I" is the first question on every return.
        if matches!(event, InputEvent::WindowFocusGained) && self.phase == SetupPhase::Waiting {
            let current = self.current_text(ctx);
            ctx.say(&format!("Back in Freight Fate. {current}"));
            return;
        }
        menu_default_handle_event(self, ctx, event);
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
        if self.phase == SetupPhase::Waiting
            && !self.still_waiting_said
            && self.poll_started.is_some_and(|started| {
                started.elapsed().as_secs_f64() > self.schedule.still_waiting_after_s
            })
        {
            self.still_waiting_said = true;
            // interrupt=False, deliberately: this fires five seconds after the
            // activation announcement *starts*, and the browser-failed variant
            // of that announcement (code, address, and both fallback menu
            // items) takes far longer than five seconds to speak. Interrupting
            // would cut a player off mid-address on exactly the path -- a
            // remote or streamed session where the browser never opens --
            // where the spoken address is the only way to finish setup.
            ctx.say_with("Still waiting.", Say::queued());
        }
        let Some(outcome) = self.outcome.take() else {
            return;
        };
        match outcome {
            SetupOutcome::StartFailed => {
                self.phase = SetupPhase::Idle;
                self.refresh(ctx, true);
                ctx.say("Could not reach orinks.net. Try again.");
            }
            SetupOutcome::Activation(activation) => {
                self.activation = Some(activation.clone());
                self.phase = SetupPhase::Waiting;
                self.still_waiting_said = false;
                self.refresh(ctx, true);
                self.announce_activation(ctx, &activation);
                // Clock starts *after* the announcement is queued, not before, so
                // the still-waiting interval measures five seconds of actual
                // waiting rather than five seconds of the announcement still
                // being spoken.
                self.poll_started = Some(Instant::now());
            }
            SetupOutcome::Ready(result) => self.finish_success(ctx, result),
            SetupOutcome::Sharing(answer) => self.finish_sharing(ctx, answer),
            SetupOutcome::Expired => {
                self.activation = None;
                self.phase = SetupPhase::Expired;
                self.refresh(ctx, true);
                ctx.say(
                    "Your activation code expired. Choose Set up this computer \
                     with orinks.net again for a new code.",
                );
            }
            SetupOutcome::Error => {
                self.activation = None;
                self.phase = SetupPhase::Error;
                self.refresh(ctx, true);
                // A 400 here means the stored device_code is malformed, so the
                // only fix is a fresh code. Heard aloud, saying "trying again
                // will not fix it" and then naming a menu item to choose again
                // reads as a contradiction -- the two halves have to agree, so
                // this names the fresh code as the fix and leaves it there.
                ctx.say(
                    "That activation code cannot be used. Choose Set up this \
                     computer with orinks.net for a fresh code.",
                );
            }
        }
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        // "starting" included, not just "waiting": a player who backs out
        // while the game is still contacting orinks.net for a code gets the
        // same confirmation as one who backs out mid-poll, rather than just
        // the generic menu-back sound and no word on what happened.
        if self.phase == SetupPhase::Sharing {
            // Nothing to cancel here -- the account is already connected, and
            // leaving mid-request would strand the game believing Profile
            // sharing is off while orinks.net may already have turned it on.
            // The request carries its own timeout, so this waits at most that
            // long. Same rule as ProfileSharingSyncState.
            ctx.say("Your account is connected. Stay here for the Profile sharing result.");
            return;
        }
        if matches!(self.phase, SetupPhase::Starting | SetupPhase::Waiting) {
            ctx.say("Setup canceled. Nothing was saved.");
        }
        menu_default_go_back(self, ctx);
    }

    fn exit(&mut self, _ctx: &mut GameContext) {
        // Stops the poll worker no matter how this state is left (Cancel,
        // Escape, or a programmatic pop elsewhere) -- backing out must never
        // leave a thread polling into a dead state.
        self.stop.set();
    }
}

impl_state_for_menu!(OnlineSetupState);

// -- ProfileSharingSyncState ------------------------------------------------------------------

/// Synchronize Profile sharing without blocking the game loop.
pub struct ProfileSharingSyncState {
    pub menu: MenuCore<Self>,
    pub enabled: bool,
    pub pending: bool,
    /// worker -> update() mailbox: `set_profile_sharing`'s answer.
    pub outcome: Mailbox<&'static str>,
    pub threaded: bool,
}

impl ProfileSharingSyncState {
    pub const TITLE: &'static str = "Profile sharing";

    /// `ProfileSharingSyncState(ctx, enabled)`.
    pub fn new(_ctx: &mut GameContext, enabled: bool) -> Self {
        Self {
            menu: MenuCore::new(Self::TITLE),
            enabled,
            pending: false,
            outcome: Mailbox::new(),
            threaded: true,
        }
    }

    pub fn start(&mut self, ctx: &mut GameContext) {
        if self.pending {
            return;
        }
        let Some(identity) = load_identity() else {
            let setup = OnlineSetupState::new(ctx);
            ctx.push_state(setup);
            return;
        };
        self.pending = true;
        if !self.enabled {
            ctx.settings.profile_sharing_pending_off = true;
            if let Err(e) = ctx.settings.save() {
                log::warn!("Could not save settings: {e}");
            }
            ctx.apply_online_presence();
        }
        self.refresh(ctx, true);
        ctx.say(if self.enabled {
            "Turning Profile sharing on."
        } else {
            "Turning Profile sharing off. Posting has stopped. Public information may stay \
             visible until orinks.net confirms."
        });
        let enabled = self.enabled;
        let outcome = self.outcome.clone();
        let transport = online_transport();
        run_worker(self.threaded, "profile-sharing", move || {
            outcome.post(online_presence::set_profile_sharing(
                &identity,
                enabled,
                transport.as_ref(),
            ));
        });
    }
}

impl Menu for ProfileSharingSyncState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let direction = if self.enabled { "on" } else { "off" };
        let action = if self.pending {
            format!("Turning Profile sharing {direction}")
        } else {
            format!("Turn Profile sharing {direction}")
        };
        vec![
            MenuItem::new(action, |s: &mut Self, ctx| s.start(ctx)),
            MenuItem::new("Hear what gets shared", |_: &mut Self, ctx| {
                ctx.say(DISCLOSURE)
            }),
            MenuItem::new("Cancel", |s: &mut Self, ctx| s.go_back(ctx)),
        ]
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
        let Some(outcome) = self.outcome.take() else {
            return;
        };
        self.pending = false;
        if outcome == "ok" {
            ctx.settings.online_presence = self.enabled;
            if self.enabled {
                ctx.settings.profile_sharing_consent_version = PROFILE_SHARING_CONSENT_VERSION;
            }
            ctx.settings.profile_sharing_pending_off = false;
            if let Err(e) = ctx.settings.save() {
                log::warn!("Could not save settings: {e}");
            }
            ctx.apply_online_presence();
            ctx.say(if self.enabled {
                "Profile sharing is on. Eligible driver information and gameplay updates can now appear publicly on orinks.net."
            } else {
                "Profile sharing is off. Posting has stopped and your Freight Fate profile and activity are no longer public."
            });
            ctx.pop_state();
            return;
        }
        self.refresh(ctx, true);
        ctx.say(if self.enabled {
            "Profile sharing is still off. orinks.net could not confirm the change. Try again."
        } else {
            "Profile sharing may still be public. Posting is stopped, but orinks.net could not \
             confirm. Turn Profile sharing off retries."
        });
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        if self.pending {
            ctx.say("Profile sharing is still updating. Stay here for the result.");
            return;
        }
        menu_default_go_back(self, ctx);
    }
}

impl_state_for_menu!(ProfileSharingSyncState);
