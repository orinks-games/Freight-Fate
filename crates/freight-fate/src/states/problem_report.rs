//! Report a problem: the logs go ahead to orinks.net on a worker thread,
//! then the report form opens in the player's web browser with them already
//! attached. The page takes what happened and a reply address.
//!
//! Two screens. [`ProblemReportLinkState`] does the send and the opening,
//! and falls back to the bug-report form on GitHub, the way the row always
//! worked, when the relay or the browser will not cooperate.
//! [`CrashReportOfferState`] is the main menu's question after a run that
//! never quit: report it, not now, or stop asking.

use std::sync::Arc;
use std::thread::JoinHandle;

use crate::app::{logging::log_folder_words, GameContext};
use crate::browser::open_url;
use crate::impl_state_for_menu;
use crate::problem_reports::{self, Opened, SendError, Sender};
use crate::states::base::{InputEvent, Key, Menu, MenuCore, MenuItem, State};
use crate::states::online_states::{run_worker, Mailbox};
use crate::updater;

/// Spoken on arrival, while the logs go ahead.
pub const PREPARING: &str = "Preparing the report.";
/// Spoken as the screen closes once the browser has the page.
pub const OPENED: &str =
    "The report opened in your browser. The game log is already attached; finish it there.";

/// Sends the logs ahead and opens the report form.
pub struct ProblemReportLinkState {
    /// Whether this report follows a run that did not quit.
    crashed: bool,
    outcome: Mailbox<Result<Opened, SendError>>,
    /// The send, when one was spawned (tests join it).
    pub worker: Option<JoinHandle<()>>,
    /// Workers on a thread (the game) or inline (tests).
    pub threaded: bool,
    /// What the window shows under the title.
    status: String,
}

impl ProblemReportLinkState {
    /// A report of a problem the player noticed, or of the last crash.
    pub fn new(crashed: bool) -> Self {
        Self {
            crashed,
            outcome: Mailbox::new(),
            worker: None,
            threaded: true,
            status: PREPARING.to_string(),
        }
    }

    /// The bug-report form on GitHub instead, saying why. The game log has
    /// to be attached by hand there, so the line says where it is.
    fn fall_back(&mut self, ctx: &mut GameContext, why: &str) {
        ctx.audio.play("ui/error");
        let line = if open_url(&problem_reports::github_issue_url()) {
            format!(
                "{why} Opening the bug report page on GitHub instead. Attach your game \
                 log: game.log in {}. If you restarted the game after the problem, \
                 attach game.prev.log, the previous run's log.",
                log_folder_words()
            )
        } else {
            format!(
                "{why} Could not open a web browser either. Report problems at \
                 github.com/{}/issues.",
                updater::REPO
            )
        };
        self.status = line.clone();
        ctx.say(&line);
        ctx.pop_state();
    }

    /// Why the form did not open, in a sentence for the player.
    fn reason(error: SendError) -> String {
        match error {
            SendError::RateLimited => "Too many reports have been sent today.".to_string(),
            SendError::ReplyAddressTakesNoMail | SendError::Rejected(_) => {
                "orinks.net did not accept the report.".to_string()
            }
            SendError::Invalid(_) => "The report could not be prepared.".to_string(),
            SendError::Failed(reason) => {
                let reason = reason.trim();
                if reason.ends_with(['.', '!', '?']) {
                    reason.to_string()
                } else {
                    format!("{reason}.")
                }
            }
        }
    }
}

impl State for ProblemReportLinkState {
    fn enter(&mut self, ctx: &mut GameContext) {
        if self.worker.is_some() || self.outcome.take().is_some() {
            // Revealed again after a screen on top; the send is already on
            // its way or already answered.
            return;
        }
        ctx.say(PREPARING);
        // Read on the game thread: a test's fake sender lives there.
        let sender: Arc<dyn Sender> = problem_reports::sender();
        let outcome = self.outcome.clone();
        let crashed = self.crashed;
        let endpoint = problem_reports::endpoint();
        self.worker = run_worker(self.threaded, "problem-report", move || {
            let report = problem_reports::draft(crashed);
            outcome.post(report.open_with(&endpoint, sender.as_ref()));
        });
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
        let Some(outcome) = self.outcome.take() else {
            return;
        };
        match outcome {
            Ok(opened) if open_url(&opened.url) => {
                self.status = OPENED.to_string();
                ctx.say(OPENED);
                ctx.pop_state();
            }
            Ok(_) => {
                // A browser that would not open the page will not open the
                // GitHub one either, so no second try.
                ctx.audio.play("ui/error");
                let line = format!(
                    "Could not open a web browser. Report problems at github.com/{}/issues.",
                    updater::REPO
                );
                self.status = line.clone();
                ctx.say(&line);
                ctx.pop_state();
            }
            Err(error) => {
                let reason = Self::reason(error);
                self.fall_back(
                    ctx,
                    &format!("The report form could not be opened: {reason}"),
                );
            }
        }
    }

    fn handle_event(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        let Some((key, _, _)) = event.key_down() else {
            return;
        };
        match key {
            // The worker keeps going; a page nobody opens expires on the site.
            Key::Escape => {
                ctx.audio.play("ui/menu_back");
                ctx.pop_state();
            }
            Key::Return | Key::KpEnter | Key::Space => ctx.say(&self.status),
            _ => {}
        }
    }

    fn lines(&self, _ctx: &GameContext) -> Vec<String> {
        vec![
            "Report a problem".to_string(),
            String::new(),
            self.status.clone(),
        ]
    }
}

/// The main menu's question after a run that never quit.
pub struct CrashReportOfferState {
    menu: MenuCore<Self>,
}

impl CrashReportOfferState {
    pub fn new() -> Self {
        Self {
            menu: MenuCore::new("Report the last crash?").with_intro_help(
                "Report it opens the report form in your web browser with the previous \
                 run's log attached. Not now closes this. Stop asking turns the question \
                 off; Settings, Problem reports turns it back on.",
            ),
        }
    }

    fn report(&mut self, ctx: &mut GameContext) {
        ctx.replace_state(ProblemReportLinkState::new(true));
    }

    fn stop_asking(&mut self, ctx: &mut GameContext) {
        ctx.settings.crash_report_offer = false;
        if let Err(e) = ctx.settings.save() {
            log::warn!("Could not save settings: {e}");
        }
        ctx.say(
            "Freight Fate will not ask again after a crash. Report a problem on the main \
             menu still works, and Settings, Problem reports turns the question back on.",
        );
        ctx.pop_state();
    }
}

impl Default for CrashReportOfferState {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu for CrashReportOfferState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let text = format!(
            "Freight Fate closed unexpectedly the last time it ran. Report the problem? {}",
            self.current_text(ctx)
        );
        ctx.say(&text);
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![
            MenuItem::new("Report it", |s: &mut Self, ctx| s.report(ctx)).help(
                "The report form opens in your web browser with the previous run's log \
                 attached. Say what you were doing when it closed.",
            ),
            MenuItem::new("Not now", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Back to the main menu. Report a problem is there whenever you want it."),
            MenuItem::new("Stop asking after crashes", |s: &mut Self, ctx| {
                s.stop_asking(ctx)
            })
            .help("Never ask again after a crash. Settings, Problem reports turns it back on."),
        ]
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        ctx.audio.play("ui/menu_back");
        ctx.pop_state();
    }
}

impl_state_for_menu!(CrashReportOfferState);
