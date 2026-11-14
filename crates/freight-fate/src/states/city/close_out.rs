//! Closing out a career that is over: the player's own act of removing the
//! save from this computer and its backups from the cloud.
//!
//! A lifetime disqualification ends the driving, not the data. The game never
//! deletes a career on its own: a mis-held key or a bug in the one code path
//! that reaches the disqualification would otherwise destroy a career with no
//! way back, and the real record outlives a real disqualification by decades.
//! So the terminal offers this screen, only to a career that is over, last on
//! the menu, behind a confirmation that says exactly what goes and what stays.
//!
//! The cloud half runs on a worker thread like every other cloud call; the
//! screen waits for its answer before touching the local file, so the player
//! hears one outcome that covers both. A computer with no cloud sign-in
//! removes the local save and says the cloud was never involved.

use std::path::PathBuf;

use ff_core::models::profile::find_save_path;

use crate::app::{GameContext, Say};
use crate::cloud_saves::{self, AUTH_HELP};
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::city::profile;
use crate::states::main_menu::MainMenuState;
use crate::states::online_states::{load_identity, run_worker, Mailbox};

/// Enter confirms, Escape keeps the career.
pub struct CloseOutCareerState {
    menu: MenuCore<Self>,
    name: String,
    path: PathBuf,
    outcome: Mailbox<String>,
    busy: bool,
    /// Tests run the cloud call inline; the game runs it on a worker.
    pub threaded: bool,
}

impl CloseOutCareerState {
    pub fn new(ctx: &GameContext) -> Self {
        let p = profile(ctx);
        Self {
            menu: MenuCore::new("Close out this career")
                .with_open_sound(Some("ui/error"))
                .with_intro_help("Enter confirms, Escape keeps the career."),
            name: p.name.clone(),
            path: p.path(),
            outcome: Mailbox::new(),
            busy: false,
            threaded: true,
        }
    }

    fn confirm(&mut self, ctx: &mut GameContext) {
        if self.busy {
            ctx.say("Still closing out. One moment.");
            return;
        }
        let Some(identity) = load_identity() else {
            self.finish(
                ctx,
                "Cloud backup was never set up on this computer, so there were no cloud \
                 backups to remove.",
            );
            return;
        };
        self.busy = true;
        ctx.say("Removing the cloud backups.");
        let save_name = self.name.clone();
        let outcome = self.outcome.clone();
        let service = ctx.cloud_saves_service().clone();
        run_worker(self.threaded, "close-out-career", move || {
            let tag =
                match cloud_saves::delete_save(&identity, &save_name, service.transport().as_ref())
                {
                    Err(cloud_saves::CloudAuthError) => "delete_auth_failed",
                    Ok(true) => {
                        service.sync_state().forget(&save_name);
                        "deleted"
                    }
                    Ok(false) => "delete_failed",
                };
            outcome.post(tag.to_string());
        });
    }

    /// The cloud has answered (or was never asked): remove the local save and
    /// leave for the title menu with one line that covers both halves.
    fn finish(&mut self, ctx: &mut GameContext, cloud_line: &str) {
        let _ = std::fs::remove_file(&self.path);
        // A legacy-format save of the same name would come back as the same
        // career on the next title screen; it goes too.
        if let Some(other) = find_save_path(&self.name) {
            if other != self.path {
                let _ = std::fs::remove_file(&other);
            }
        }
        if ctx.profile.as_ref().is_some_and(|p| p.name == self.name) {
            ctx.profile = None;
        }
        let name = self.name.clone();
        ctx.reset_to(MainMenuState::new());
        ctx.say_with(
            format!("{name} closed out. The save is gone from this computer. {cloud_line}"),
            Say::new(),
        );
    }
}

impl Menu for CloseOutCareerState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let text = format!(
            "Close out {}. This removes the save from this computer and every cloud backup \
             of it from your orinks.net account, for good. Your achievements and road journal \
             stay on your profile. {}",
            self.name,
            self.current_text(ctx)
        );
        ctx.say_with(text, Say::new().review(false));
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![
            MenuItem::new(
                format!("Yes, close out {}", self.name),
                |s: &mut Self, ctx| s.confirm(ctx),
            )
            .help("Confirm. The save and its cloud backups are removed."),
            MenuItem::new("No, keep this career", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Back to the terminal, nothing changed."),
        ]
    }

    fn update(&mut self, ctx: &mut GameContext, _dt: f64) {
        if !self.busy {
            return;
        }
        let Some(tag) = self.outcome.take() else {
            return;
        };
        self.busy = false;
        let cloud_line = match tag.as_str() {
            "deleted" => "Every cloud backup of it was removed from your orinks.net account.",
            "delete_auth_failed" => {
                // Owned: the auth help is a constant, but the sentence around
                // it is built here.
                return self.finish(
                    ctx,
                    &format!(
                        "{AUTH_HELP} The cloud backups were not removed. The Cloud saves menu \
                         can remove them once this computer is signed in again."
                    ),
                );
            }
            _ => {
                "The site could not be reached, so the cloud backups are still there. The \
                  Cloud saves menu can remove them later."
            }
        };
        self.finish(ctx, cloud_line);
    }
}

impl_state_for_menu!(CloseOutCareerState);
