//! Choosing, resetting and deleting saved careers (the `LoadDriverState`,
//! `ManageCareersState`, `CareerActionsState` and `ConfirmCareerActionState`
//! classes of `main_menu.py`).

use std::path::PathBuf;

use ff_core::models::profile::{find_save_path, LegacyCareerError, Profile};
use ff_core::models::start_options::{apply_start_option, option_for_profile};
use ff_core::playtest_levers::apply_continue_levers;
use ff_core::pyfmt::fmt_grouped;

use crate::app::{GameContext, Say};
use crate::cloud_saves::{self, AUTH_HELP};
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::main_menu::{
    career_location, career_summary, legacy_saves, loadable_saves, pending_notice_state,
    world_entry_state, MainMenuState,
};
use crate::states::online_states::{load_identity, run_worker, Mailbox};
use crate::states::save_notice::LegacyCareerNoticeState;

pub struct LoadDriverState {
    menu: MenuCore<Self>,
}

impl LoadDriverState {
    pub fn new() -> Self {
        Self {
            menu: MenuCore::new("Choose career")
                .with_intro_help("Up and Down pick a career, Enter loads it, Escape goes back."),
        }
    }

    fn explain_legacy(&mut self, ctx: &mut GameContext, legacy: &LegacyCareerError) {
        ctx.push_state(LegacyCareerNoticeState::new(&legacy.name));
    }

    fn pick(&mut self, ctx: &mut GameContext, profile: &Profile) {
        ctx.profile = Some(profile.clone());
        let lever_notes = apply_continue_levers(ctx);
        ctx.say(&format!("Welcome back, {}.", profile.name));
        // The welcome above must be heard in full before the city menu's own
        // "Parked at..." announcement -- see world_entry_state.
        let next = pending_notice_state(ctx).unwrap_or_else(|| world_entry_state(ctx, true));
        ctx.replace_shared_with(next, true, true);
        for note in lever_notes {
            ctx.say_with(note, Say::queued());
        }
    }
}

impl Default for LoadDriverState {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu for LoadDriverState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items = Vec::new();
        for (path, profile) in loadable_saves() {
            let label = career_summary(ctx, &path, &profile, true);
            let help = format!("Load {}, {}.", profile.name, career_location(ctx, &profile));
            items.push(
                MenuItem::new(label, move |s: &mut Self, ctx| s.pick(ctx, &profile)).help(help),
            );
        }
        // Careers the 1.9 load gate refused stay on the list with a spoken
        // label -- silently dropping them reads as data loss. Picking one
        // opens the notice that explains and offers a fresh start.
        for legacy in legacy_saves() {
            items.push(
                MenuItem::new(
                    format!(
                        "{}: career from an earlier version of Freight Fate",
                        legacy.name
                    ),
                    move |s: &mut Self, ctx| s.explain_legacy(ctx, &legacy),
                )
                .help(
                    "This career cannot continue in version 1.9. Enter \
                     explains and offers a new career; the save is not touched.",
                ),
            );
        }
        items.push(MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)));
        items
    }
}

impl_state_for_menu!(LoadDriverState);

pub struct ManageCareersState {
    menu: MenuCore<Self>,
}

impl ManageCareersState {
    pub fn new() -> Self {
        Self {
            menu: MenuCore::new("Manage careers").with_intro_help(
                "Up and Down pick a career, Enter opens reset and delete, Escape goes back.",
            ),
        }
    }
}

impl Default for ManageCareersState {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu for ManageCareersState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items = Vec::new();
        for (path, profile) in loadable_saves() {
            let label = career_summary(ctx, &path, &profile, true);
            let help = format!(
                "Manage {}. Reset starts the career over; delete removes the save.",
                profile.name
            );
            items.push(
                MenuItem::new(label, move |_s: &mut Self, ctx| {
                    ctx.push_state(CareerActionsState::new(path.clone(), profile.clone()))
                })
                .help(help),
            );
        }
        // A career from an earlier version cannot load, so it cannot be
        // reset either, but the player can still clear it off the list.
        for legacy in legacy_saves() {
            let Some(path) = legacy.path.clone().or_else(|| find_save_path(&legacy.name)) else {
                continue;
            };
            let name = legacy.name.clone();
            items.push(
                MenuItem::new(
                    format!("{name}: career from an earlier version of Freight Fate"),
                    move |_s: &mut Self, ctx| {
                        ctx.push_state(ConfirmCareerActionState::delete_legacy(path.clone(), &name))
                    },
                )
                .help("Enter offers to delete this save. It still works in Freight Fate 1.8."),
            );
        }
        items.push(MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)));
        items
    }
}

impl_state_for_menu!(ManageCareersState);

/// Which destructive action a confirmation screen is guarding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CareerAction {
    Reset,
    Delete,
}

impl CareerAction {
    /// `_action_label`.
    pub fn label(self) -> &'static str {
        match self {
            CareerAction::Reset => "reset",
            CareerAction::Delete => "delete",
        }
    }
}

pub struct CareerActionsState {
    menu: MenuCore<Self>,
    pub path: PathBuf,
    pub profile: Profile,
}

impl CareerActionsState {
    pub fn new(path: PathBuf, profile: Profile) -> Self {
        Self {
            menu: MenuCore::new("Career actions")
                .with_intro_help("Reset and delete both ask for confirmation. Escape goes back."),
            path,
            profile,
        }
    }

    fn confirm(&mut self, ctx: &mut GameContext, action: CareerAction) {
        ctx.push_state(ConfirmCareerActionState::new(
            self.path.clone(),
            self.profile.clone(),
            action,
        ));
    }
}

impl Menu for CareerActionsState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let text = format!(
            "Actions for {}. {}",
            career_summary(ctx, &self.path, &self.profile, true),
            self.current_text(ctx)
        );
        ctx.say_with(text, Say::queued().review(false));
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![
            MenuItem::new("Reset this career", |s: &mut Self, ctx| {
                s.confirm(ctx, CareerAction::Reset)
            })
            .help(
                "Starts over with a fresh truck, money, career stats, market, \
                 and hours clock.",
            ),
            MenuItem::new("Delete this career", |s: &mut Self, ctx| {
                s.confirm(ctx, CareerAction::Delete)
            })
            .help("Removes this saved career for good."),
            MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)),
        ]
    }
}

impl_state_for_menu!(CareerActionsState);

pub struct ConfirmCareerActionState {
    menu: MenuCore<Self>,
    pub path: PathBuf,
    pub name: String,
    /// The loaded career; `None` for a save from an earlier version, which
    /// can only be deleted.
    pub profile: Option<Profile>,
    pub action: CareerAction,
    outcome: Mailbox<String>,
    busy: bool,
    /// Tests run the cloud call inline; the game runs it on a worker.
    pub threaded: bool,
}

impl ConfirmCareerActionState {
    pub fn new(path: PathBuf, profile: Profile, action: CareerAction) -> Self {
        let name = profile.name.clone();
        Self::build(path, name, Some(profile), action)
    }

    /// Deleting a career from an earlier version, which cannot load.
    pub fn delete_legacy(path: PathBuf, name: &str) -> Self {
        Self::build(path, name.to_string(), None, CareerAction::Delete)
    }

    fn build(path: PathBuf, name: String, profile: Option<Profile>, action: CareerAction) -> Self {
        Self {
            menu: MenuCore::new("Confirm career action")
                .with_open_sound(Some("ui/error"))
                .with_intro_help("Enter confirms, Escape cancels."),
            path,
            name,
            profile,
            action,
            outcome: Mailbox::new(),
            busy: false,
            threaded: true,
        }
    }

    /// Whether this computer has backed the career up to orinks.net, so a
    /// delete has cloud backups to keep or remove.
    fn has_cloud_backups(&self, ctx: &GameContext) -> bool {
        load_identity().is_some()
            && !ctx
                .cloud_saves_service()
                .sync_state()
                .slot(&self.name)
                .is_empty()
    }

    fn reset(&mut self, ctx: &mut GameContext) {
        let Some(old) = &self.profile else {
            return;
        };
        let name = self.name.clone();
        let mut fresh = Profile::named_in(&name, &old.current_city);
        apply_start_option(&mut fresh, option_for_profile(old));
        match fresh.save() {
            // A career loaded from a file named apart from it: the reset went
            // to the file its name points at, so the old career goes.
            Ok(written) if written != self.path => {
                if let Err(e) = std::fs::remove_file(&self.path) {
                    log::warn!("Could not remove {}: {e}", self.path.display());
                }
            }
            Ok(_) => {}
            Err(e) => log::error!("Could not save the profile: {e}"),
        }
        // The title menu may still hold the old career from the last drive.
        if ctx.profile.as_ref().is_some_and(|p| p.name == name) {
            ctx.profile = None;
        }
        let message = format!(
            "{name} reset. The career starts over at {} with {} and {} dollars.",
            ctx.world.spoken_city(&fresh.current_city, None),
            fresh.carrier_name,
            fmt_grouped(fresh.money(), 0)
        );
        ctx.reset_to(MainMenuState::new());
        ctx.say(&message);
    }

    /// Delete the save here; with `with_cloud`, ask orinks.net to remove its
    /// backups first, so the player hears one outcome that covers both.
    fn delete(&mut self, ctx: &mut GameContext, with_cloud: bool) {
        if self.busy {
            ctx.say("Still deleting. One moment.");
            return;
        }
        let identity = if with_cloud { load_identity() } else { None };
        let Some(identity) = identity else {
            let line = if self.has_cloud_backups(ctx) {
                "Its cloud backups were kept, and Cloud backup on the Online menu can restore it."
            } else {
                ""
            };
            return self.finish_delete(ctx, line);
        };
        self.busy = true;
        ctx.say("Removing the cloud backups.");
        let save_name = self.name.clone();
        let outcome = self.outcome.clone();
        let transport = ctx.cloud_saves_service().transport().clone();
        run_worker(self.threaded, "career-delete-cloud", move || {
            let tag = match cloud_saves::delete_save(&identity, &save_name, transport.as_ref()) {
                Err(cloud_saves::CloudAuthError) => "delete_auth_failed",
                Ok(true) => "deleted",
                Ok(false) => "delete_failed",
            };
            outcome.post(tag.to_string());
        });
    }

    fn finish_delete(&mut self, ctx: &mut GameContext, cloud_line: &str) {
        let name = self.name.clone();
        if let Err(e) = std::fs::remove_file(&self.path) {
            if self.path.exists() {
                log::error!("Could not delete {}: {e}", self.path.display());
                ctx.reset_to(MainMenuState::new());
                ctx.say(&format!(
                    "{name} could not be deleted. The save is still on this computer."
                ));
                return;
            }
        }
        // Whatever the cloud still holds under this name belongs to a career
        // that is gone from here. Forgetting this computer's record of it
        // means a new career with the same name meets those backups as a
        // choice, instead of quietly uploading over them as their next
        // revision.
        ctx.cloud_saves_service().sync_state().forget(&name);
        if ctx.profile.as_ref().is_some_and(|p| p.name == name) {
            ctx.profile = None;
        }
        let message = if cloud_line.is_empty() {
            format!("{name} deleted.")
        } else {
            format!("{name} deleted from this computer. {cloud_line}")
        };
        ctx.reset_to(MainMenuState::new());
        ctx.say(&message);
    }
}

impl Menu for ConfirmCareerActionState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let detail = match (&self.profile, self.action) {
            (Some(profile), CareerAction::Reset) => format!(
                "Reset starts over at {} with a fresh truck, starting money, \
                 no active trip, and no delivery history.",
                ctx.world.spoken_city(&profile.current_city, None)
            ),
            _ if self.has_cloud_backups(ctx) => {
                "Delete removes this saved career from this computer for good. It also has \
                 cloud backups on your orinks.net account; choose whether they go too."
                    .to_string()
            }
            _ => "Delete removes this saved career for good.".to_string(),
        };
        let text = format!(
            "Confirm {} for {}. {detail} {}",
            self.action.label(),
            self.name,
            self.current_text(ctx)
        );
        ctx.say_with(text, Say::new().review(false));
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let name = self.name.clone();
        let mut items = Vec::new();
        match self.action {
            CareerAction::Reset => items.push(
                MenuItem::new(format!("Yes, reset {name}"), |s: &mut Self, ctx| {
                    s.reset(ctx)
                })
                .help("Confirm and reset this saved career."),
            ),
            CareerAction::Delete if self.has_cloud_backups(ctx) => {
                items.push(
                    MenuItem::new(
                        format!("Yes, delete {name} and its cloud backups"),
                        |s: &mut Self, ctx| s.delete(ctx, true),
                    )
                    .help(
                        "Removes the save from this computer and every cloud backup of it \
                         from your orinks.net account.",
                    ),
                );
                items.push(
                    MenuItem::new(
                        format!("Yes, delete {name} from this computer only"),
                        |s: &mut Self, ctx| s.delete(ctx, false),
                    )
                    .help(
                        "The cloud backups stay, and Cloud backup on the Online menu can \
                         restore the career.",
                    ),
                );
            }
            CareerAction::Delete => items.push(
                MenuItem::new(format!("Yes, delete {name}"), |s: &mut Self, ctx| {
                    s.delete(ctx, false)
                })
                .help("Confirm and delete this saved career."),
            ),
        }
        items.push(
            MenuItem::new("No, keep this career", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Back, nothing changed."),
        );
        items
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
        if !self.busy {
            return;
        }
        let Some(tag) = self.outcome.take() else {
            return;
        };
        self.busy = false;
        let cloud_line = match tag.as_str() {
            "deleted" => {
                "Every cloud backup of it was removed from your orinks.net account.".to_string()
            }
            "delete_auth_failed" => format!(
                "{AUTH_HELP} The cloud backups were not removed; Cloud backup on the Online \
                 menu can remove them once this computer is signed in again."
            ),
            _ => "The site could not be reached, so the cloud backups are still there; Cloud \
                  backup on the Online menu can remove them later."
                .to_string(),
        };
        self.finish_delete(ctx, &cloud_line);
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        if self.busy {
            ctx.say("Still deleting. One moment.");
            return;
        }
        ctx.audio.play("ui/menu_back");
        ctx.pop_state();
    }
}

impl_state_for_menu!(ConfirmCareerActionState);
