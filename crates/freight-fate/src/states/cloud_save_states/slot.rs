//! One cloud slot's actions: restore, conflict resolution, the public career
//! choice, and delete (the `CloudSlotState` half of
//! `freight_fate/states/cloud_save_states.py`, split out for length).

use std::path::PathBuf;

use ff_core::models::profile::{find_save_path, is_pre_1_9_save, LoadError, Profile};
use serde_json::{Map, Value};

use crate::app::{GameContext, SharedState};
use crate::cloud_saves::{
    self, eviction_status, rejection_status, save_slot_name, DownloadError, RestoreError,
    RestoreHooks, AUTH_HELP,
};
use crate::impl_state_for_menu;
use crate::states::base::{Label, Menu, MenuCore, MenuItem};
use crate::states::city::BACKUP_RESULT_WAIT_S;
use crate::states::online_states::{load_identity, menu_default_go_back, run_worker, Mailbox};

use super::{
    backed_up_text, created_at_ms, install_restored_profile, is_legacy_snapshot, local_summary_for,
    summary_of, CloudBackupState, ConfirmDeleteCloudState, ConfirmKeepMineState,
    ConfirmPublicCareerState, ConfirmRestoreState, LEGACY_BACKUP_NOTICE,
};

/// The slot screen a confirmation reports back to: the shared state on the
/// stack plus the career name captured when the confirmation was pushed.
/// The slot is borrowed for its own handler while a confirmation is being
/// entered, so nothing may read it then; the yes-answer borrows it after
/// the confirmation has popped itself.
#[derive(Clone)]
pub struct SlotHandle {
    state: SharedState,
    pub save_name: String,
}

impl SlotHandle {
    pub fn new(state: SharedState, save_name: &str) -> Self {
        Self {
            state,
            save_name: save_name.to_string(),
        }
    }

    /// Run `f` on the slot screen (`self._slot_state.start_*()`).
    pub fn with(
        &self,
        ctx: &mut GameContext,
        f: impl FnOnce(&mut CloudSlotState, &mut GameContext),
    ) {
        match self.state.try_borrow_mut() {
            Ok(mut state) => match state.as_any_mut().downcast_mut::<CloudSlotState>() {
                Some(slot) => f(slot, ctx),
                None => log::error!("a cloud confirmation's slot handle is not a CloudSlotState"),
            },
            Err(_) => log::error!("a cloud confirmation answered while its slot was borrowed"),
        }
    }
}

/// Actions for one cloud slot: restore the latest backup, restore an
/// older one, resolve a conflict by choosing which copy wins, or delete
/// every cloud backup of the career (the accidental-upload escape hatch).
///
/// Restores overwrite the local save file for this career; the previous
/// local file is kept beside it and the choice is confirmed before anything
/// is touched. The download and any upload run on a worker thread; a small
/// mailbox hands the outcome back to `update` for speech.
pub struct CloudSlotState {
    pub menu: MenuCore<Self>,
    pub save_name: String,
    /// newest first, from the list fetch
    pub revisions: Vec<Value>,
    pub is_public: bool,
    /// The backup list underneath, told when the public career changes.
    on_public_chosen: Option<SharedState>,
    pub busy: bool,
    /// worker -> update() mailbox: the outcome tags of the Python state.
    pub outcome: Mailbox<String>,
    pub restored_path: Option<PathBuf>,
    /// worker -> update() for the restore path, beside the outcome tag.
    restored: Mailbox<PathBuf>,
    /// A "Back up this career now" attempt in flight: the slot it went up
    /// under, the queue's attempt token, and the real seconds left to wait
    /// for a result before the player is told it is still trying.
    backup_watch: Option<(String, i64, f64)>,
    pub status: String,
    /// Whether the restore in flight replaces a save on this computer, which
    /// is then kept beside it as a fallback file.
    replaces_local: bool,
    pub threaded: bool,
}

impl CloudSlotState {
    /// `CloudSlotState(ctx, save_name, revisions, public_save_name=...,
    /// on_public_chosen=...)`.
    pub fn new(
        _ctx: &mut GameContext,
        save_name: &str,
        revisions: Vec<Value>,
        public_save_name: Option<&str>,
        on_public_chosen: Option<SharedState>,
    ) -> Self {
        Self {
            menu: MenuCore::new(&format!("Cloud backup: {save_name}")),
            save_name: save_name.to_string(),
            revisions,
            is_public: public_save_name == Some(save_name),
            on_public_chosen,
            busy: false,
            outcome: Mailbox::new(),
            restored_path: None,
            restored: Mailbox::new(),
            backup_watch: None,
            status: "Ready.".to_string(),
            replaces_local: false,
            threaded: true,
        }
    }

    /// A handle for the confirmation screens this slot pushes. The slot is
    /// the top of the stack while its own handler runs, so `ctx.state()` is
    /// it.
    pub fn handle(&self, ctx: &GameContext) -> SlotHandle {
        SlotHandle::new(
            ctx.state().expect("the slot screen is on the stack"),
            &self.save_name,
        )
    }

    /// The recorded conflict for this slot, when one is waiting.
    pub fn conflict(&self, ctx: &GameContext) -> Option<Map<String, Value>> {
        ctx.cloud_saves_service()
            .conflicts()
            .remove(&self.save_name)
    }

    fn restore_label(&self) -> String {
        if self.busy {
            return "Working on it".to_string();
        }
        let Some(latest) = self.revisions.first() else {
            return "No backups for this career yet".to_string();
        };
        let legacy = if is_legacy_snapshot(latest) {
            ", from an earlier version of Freight Fate"
        } else {
            ""
        };
        format!(
            "Restore the latest backup, {}{legacy}",
            backed_up_text(created_at_ms(latest))
        )
    }

    /// Both copies, described the same way, before either choice is read.
    ///
    /// Naming only the cloud side made the decision unanswerable: the player
    /// could hear what he would be moving TO but nothing about what he would
    /// be giving up.
    fn conflict_label(&self, conflict: &Map<String, Value>) -> String {
        let summary = conflict
            .get("latestSummary")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty());
        let mine = self.local_summary();
        let mut bits = vec![
            "This career needs attention: the cloud copy changed on another computer.".to_string(),
        ];
        if !mine.is_empty() {
            bits.push(format!("This computer's copy is {mine}."));
        }
        if let Some(summary) = summary {
            bits.push(format!("The cloud copy is {summary}."));
        }
        bits.push("The choices below pick which copy to keep.".to_string());
        bits.join(" ")
    }

    // -- actions ----------------------------------------------------------------

    fn say_busy(ctx: &mut GameContext) {
        ctx.say("Still working on the last choice.");
    }

    fn confirm_restore(&mut self, ctx: &mut GameContext, entry: Option<&Value>) {
        if self.busy {
            Self::say_busy(ctx);
            return;
        }
        let Some(entry) = entry else {
            ctx.say("There is no backup to restore for this career.");
            return;
        };
        if is_legacy_snapshot(entry) {
            // Refused before the confirmation step: there is no yes to offer.
            // Nothing was downloaded and nothing on either side changes.
            ctx.say(LEGACY_BACKUP_NOTICE);
            return;
        }
        let handle = self.handle(ctx);
        let confirm = ConfirmRestoreState::new(ctx, handle, entry.clone());
        ctx.push_state(confirm);
    }

    /// Called by the confirmation state after the player says yes.
    pub fn start_restore(&mut self, ctx: &mut GameContext, entry: &Value) {
        let Some(identity) = load_identity() else {
            ctx.say("Cloud backup is not set up on this computer.");
            return;
        };
        self.busy = true;
        self.replaces_local = self.has_local_save();
        self.refresh(ctx, true);
        ctx.say("Downloading the backup.");
        let revision = match entry.get("revision") {
            Some(Value::Number(n)) if n.is_i64() || n.is_u64() => n.as_i64(),
            _ => None,
        };
        let save_name = self.save_name.clone();
        let outcome = self.outcome.clone();
        let service = ctx.cloud_saves_service().clone();
        let restored_slot = self.restored.clone();
        run_worker(self.threaded, "cloud-saves-restore", move || {
            let payload = match cloud_saves::download_save(
                &identity,
                &save_name,
                revision,
                service.transport().as_ref(),
                None,
            ) {
                Err(DownloadError::Integrity(error)) => {
                    outcome.post(error.code);
                    return;
                }
                Err(DownloadError::Auth(_)) => {
                    outcome.post("auth_failed".to_string());
                    return;
                }
                Ok(None) => {
                    outcome.post("download_failed".to_string());
                    return;
                }
                Ok(Some(payload)) => payload,
            };
            let is_legacy = |profile: &Value| match profile {
                Value::Object(map) => is_pre_1_9_save(map),
                _ => true,
            };
            let write = install_restored_profile;
            let hooks = RestoreHooks {
                is_legacy: &is_legacy,
                write: &write,
            };
            match cloud_saves::restore_to_disk(&payload, Some(service.sync_state()), &hooks, None) {
                // The metadata gate above missed it (an entry without a save
                // version); restore_to_disk checked the downloaded profile
                // itself and refused before touching disk.
                Err(RestoreError::LegacyCareer(_)) => outcome.post("legacy_refused".to_string()),
                Err(_) => outcome.post("restore_failed".to_string()),
                Ok(path) => {
                    restored_slot.post(path);
                    outcome.post("restored".to_string());
                }
            }
        });
    }

    fn confirm_keep_mine(&mut self, ctx: &mut GameContext) {
        if self.busy {
            Self::say_busy(ctx);
            return;
        }
        let handle = self.handle(ctx);
        let confirm = ConfirmKeepMineState::new(ctx, handle);
        ctx.push_state(confirm);
    }

    fn confirm_delete(&mut self, ctx: &mut GameContext) {
        if self.busy {
            Self::say_busy(ctx);
            return;
        }
        let handle = self.handle(ctx);
        let confirm = ConfirmDeleteCloudState::new(ctx, handle);
        ctx.push_state(confirm);
    }

    fn confirm_public(&mut self, ctx: &mut GameContext) {
        if self.busy {
            Self::say_busy(ctx);
            return;
        }
        let handle = self.handle(ctx);
        let confirm = ConfirmPublicCareerState::new(ctx, handle);
        ctx.push_state(confirm);
    }

    /// Called by the confirmation state after the player says yes.
    pub fn start_set_public(&mut self, ctx: &mut GameContext) {
        let Some(identity) = load_identity() else {
            ctx.say("Cloud backup is not set up on this computer.");
            return;
        };
        self.busy = true;
        self.refresh(ctx, true);
        ctx.say("Telling orinks.net.");
        let save_name = self.save_name.clone();
        let outcome = self.outcome.clone();
        let transport = ctx.cloud_saves_service().transport().clone();
        run_worker(self.threaded, "cloud-saves-public", move || {
            let tag =
                match cloud_saves::set_public_save(&identity, Some(&save_name), transport.as_ref())
                {
                    Err(cloud_saves::CloudAuthError) => "public_auth_failed",
                    Ok(true) => "public_set",
                    Ok(false) => "public_failed",
                };
            outcome.post(tag.to_string());
        });
    }

    fn has_local_save(&self) -> bool {
        find_save_path(&self.save_name).is_some()
    }

    /// This computer's copy in the same shape the cloud copy is described.
    ///
    /// The conflict screen used to name the cloud copy's level and money and
    /// say nothing at all about the save already on the machine, so the
    /// player was asked to choose between something described and something
    /// anonymous -- and the safe-feeling answer to that is to choose
    /// neither, which is what Brandon did for a day (owner, 2026-08-15).
    fn local_summary(&self) -> String {
        local_summary_for(&self.save_name)
    }

    /// Called by the confirmation state after the player says yes.
    pub fn start_delete(&mut self, ctx: &mut GameContext) {
        let Some(identity) = load_identity() else {
            ctx.say("Cloud backup is not set up on this computer.");
            return;
        };
        self.busy = true;
        self.refresh(ctx, true);
        ctx.say("Deleting the cloud backups.");
        let save_name = self.save_name.clone();
        let outcome = self.outcome.clone();
        let service = ctx.cloud_saves_service().clone();
        run_worker(self.threaded, "cloud-saves-delete", move || {
            let tag =
                match cloud_saves::delete_save(&identity, &save_name, service.transport().as_ref())
                {
                    Err(cloud_saves::CloudAuthError) => "delete_auth_failed",
                    Ok(true) => {
                        // Forget the slot (conflict included) so the next local save
                        // starts a fresh slot instead of naming a dead revision.
                        service.sync_state().forget(&save_name);
                        "deleted"
                    }
                    Ok(false) => "delete_failed",
                };
            outcome.post(tag.to_string());
        });
    }

    pub fn start_keep_mine(&mut self, ctx: &mut GameContext) {
        if self.busy {
            Self::say_busy(ctx);
            return;
        }
        let loaded = match find_save_path(&self.save_name) {
            None => Err(None),
            Some(path) => Profile::load(&path).map_err(Some),
        };
        let profile_dict = match loaded {
            Ok(profile) => Value::Object(profile.to_dict()),
            Err(Some(LoadError::LegacyCareer(_))) => {
                ctx.say(
                    "This computer's save is from an earlier version of Freight Fate and \
                     cannot be uploaded. The cloud copy can still be used.",
                );
                return;
            }
            Err(_) => {
                ctx.say(
                    "This computer's save could not be read, so it cannot be uploaded. The \
                     cloud copy can still be used.",
                );
                return;
            }
        };
        self.busy = true;
        self.refresh(ctx, true);
        ctx.say("Backing up this computer's save.");
        let save_name = self.save_name.clone();
        let outcome = self.outcome.clone();
        let service = ctx.cloud_saves_service().clone();
        run_worker(self.threaded, "cloud-saves-keep-mine", move || {
            let result = service.resolve_keep_mine(&save_name, &profile_dict);
            // "ok" -> kept_mine; otherwise the classified failure family
            // (network/auth/rejected/conflict) picks which honest line
            // update() speaks -- never the same "check your connection"
            // line for every cause (Jessie's report, 2026-08-14). A rejected
            // upload comes back as "rejected:<reason>", carrying the raw
            // reason code so update() can name the career and split the
            // story by cause the same way the background queue does
            // (Shane's report, 2026-08-14).
            outcome.post(if result == "ok" {
                "kept_mine".to_string()
            } else {
                format!("keep_mine_failed_{result}")
            });
        });
    }

    /// The always-present upload (Brandon, 2026-08-15). A career used to
    /// travel upward two ways only: the queue after a save, and the
    /// conflict screen's "Keep this computer's save and back it up" -- which
    /// vanished with the conflict that summoned it, so a stuck queue left no
    /// way to send a career by hand and he lost a level to the cloud's older
    /// copy. Same upload as the background queue, asked for now, with the
    /// result spoken here; `update` watches for it the way the terminal's
    /// Save game does.
    pub fn start_backup_now(&mut self, ctx: &mut GameContext) {
        if self.busy {
            Self::say_busy(ctx);
            return;
        }
        let service = ctx.cloud_saves_service().clone();
        if load_identity().is_none() {
            ctx.say("Cloud backup is not set up on this computer.");
            return;
        }
        if !service.enabled() {
            // An account, but backups switched off: the standing line says
            // where the switch is.
            ctx.say(&service.status());
            return;
        }
        let loaded = match find_save_path(&self.save_name) {
            None => Err(None),
            Some(path) => Profile::load(&path).map_err(Some),
        };
        let profile = match loaded {
            Ok(profile) => profile,
            Err(Some(LoadError::LegacyCareer(_))) => {
                ctx.say(
                    "This computer's save is from an earlier version of Freight Fate and \
                     cannot be backed up. The save stays as it is.",
                );
                return;
            }
            Err(_) => {
                ctx.say("This computer's save could not be read, so it cannot be backed up.");
                return;
            }
        };
        let snapshot = Value::Object(profile.to_dict());
        let Some(token) = service.backup_now(&profile.name, snapshot) else {
            ctx.say("Cloud backup is not set up on this computer.");
            return;
        };
        self.busy = true;
        self.backup_watch = Some((save_slot_name(&profile.name), token, BACKUP_RESULT_WAIT_S));
        self.status = "Backing up this career.".to_string();
        self.refresh(ctx, true);
        ctx.say("Backing up this career.");
    }

    /// One spoken line per outcome of a by-hand backup, in the same words
    /// the terminal's Save game and the background queue use for the same
    /// outcome. "Nothing was changed" every time it did not go through: the
    /// row's whole promise is that pressing it can never cost the career.
    fn speak_backup_outcome(&mut self, ctx: &mut GameContext, outcome: &str) {
        let name = self.save_name.clone();
        match outcome {
            "accepted" => {
                self.status = "Backed up. The cloud copy matches this computer's save.".to_string();
                ctx.audio.play("ui/menu_select");
                ctx.say("Backed up. The cloud copy now matches this computer's save.");
            }
            evicted if evicted.starts_with("accepted:evicted:") => {
                let gone = evicted
                    .strip_prefix("accepted:evicted:")
                    .unwrap_or_default();
                self.status = "Backed up. The cloud copy matches this computer's save.".to_string();
                ctx.audio.play("ui/menu_select");
                ctx.say(&format!(
                    "Backed up. The cloud copy now matches this computer's save. {}",
                    eviction_status(gone)
                ));
            }
            "unchanged" => {
                self.status =
                    "Already backed up. The cloud copy matches this computer's save.".to_string();
                ctx.say("Already backed up. The cloud copy matches this computer's save.");
            }
            "conflict" => {
                self.status =
                    "The cloud copy changed on another computer. Nothing was changed.".to_string();
                ctx.say(
                    "The cloud copy changed on another computer, so nothing was \
                     changed. Choose which copy to keep from the rows below.",
                );
            }
            "auth" => {
                self.status = "Reconnect needed. Nothing was changed.".to_string();
                ctx.say(&format!("{AUTH_HELP} Nothing was changed."));
            }
            rejected if rejected.starts_with("rejected:") => {
                let reason = rejected.strip_prefix("rejected:").unwrap_or_default();
                let message = rejection_status(&name, Some(reason));
                self.status = format!("{name}: backup not accepted. Nothing was changed.");
                ctx.say(&format!("{message} Nothing was changed."));
            }
            _ => {
                self.status =
                    "The backup has not gone through yet. Still trying in the background."
                        .to_string();
                ctx.say(
                    "The backup has not gone through yet. The game keeps trying in the \
                     background, and the cloud copy was not changed.",
                );
            }
        }
    }

    /// If the restored career is the one currently loaded, re-read it so
    /// a later save cannot overwrite the restore with stale memory.
    fn reload_active_profile(&mut self, ctx: &mut GameContext) {
        let Some(profile) = &ctx.profile else {
            return;
        };
        if save_slot_name(&profile.name) != self.save_name {
            return;
        }
        ctx.profile = Profile::load(&profile.path()).ok();
    }

    fn speak_outcome(&mut self, ctx: &mut GameContext, outcome: &str) {
        match outcome {
            "restored" if !self.replaces_local => {
                self.status = "Backup restored and verified.".to_string();
                ctx.audio.play("ui/menu_select");
                ctx.say(&format!(
                    "Backup restored. {} is back on this computer.",
                    self.save_name
                ));
            }
            "restored" => {
                self.status =
                    "Backup restored and verified. The replaced save was kept.".to_string();
                self.reload_active_profile(ctx);
                ctx.audio.play("ui/menu_select");
                ctx.say(&format!(
                    "Backup restored. {} on this computer now \
                     matches the cloud copy, and the save it replaced was kept \
                     beside it as a fallback file.",
                    self.save_name
                ));
            }
            "kept_mine" => {
                self.status = "This computer's save is now the accepted cloud backup.".to_string();
                ctx.audio.play("ui/menu_select");
                ctx.say(
                    "Done. The cloud copy now matches this computer's save, and \
                     backups for this career are on again.",
                );
            }
            "deleted" => {
                self.revisions = Vec::new();
                self.status = "Cloud backups deleted for this career.".to_string();
                let local = if self.has_local_save() {
                    " Your save on this computer was not touched."
                } else {
                    ""
                };
                ctx.audio.play("ui/menu_select");
                ctx.say(&format!(
                    "Deleted. Every cloud backup of {} was \
                     removed from your orinks.net account.{local}",
                    self.save_name
                ));
            }
            "delete_failed" => {
                self.status = "Delete failed. The cloud backups were not changed.".to_string();
                ctx.say("The delete did not go through. The cloud backups were not changed.");
            }
            "delete_auth_failed" => {
                self.status = "Reconnect needed. Nothing was deleted.".to_string();
                ctx.say(&format!(
                    "{AUTH_HELP} Nothing was deleted, and the cloud \
                     backups were not changed."
                ));
            }
            "public_set" => {
                self.is_public = true;
                if let Some(list) = &self.on_public_chosen {
                    if let Ok(mut state) = list.try_borrow_mut() {
                        if let Some(list) = state.as_any_mut().downcast_mut::<CloudBackupState>() {
                            list.public_career_chosen(ctx, &self.save_name);
                        }
                    }
                }
                self.status = "This is now your public career.".to_string();
                ctx.audio.play("ui/menu_select");
                ctx.say(&format!(
                    "Done. {} is now your public career. Your other careers stay private \
                     cloud backups.",
                    self.save_name
                ));
            }
            "public_failed" => {
                self.status =
                    "The public career choice did not go through. Nothing changed.".to_string();
                ctx.say("The choice did not go through. Your public career is unchanged.");
            }
            "public_auth_failed" => {
                self.status = "Reconnect needed. Your public career is unchanged.".to_string();
                ctx.say(&format!("{AUTH_HELP} Your public career is unchanged."));
            }
            "keep_mine_failed_network" => {
                self.status = "Cloud overwrite failed. Nothing was changed.".to_string();
                ctx.say(
                    "The upload did not go through. Check your connection. Nothing was changed.",
                );
            }
            "keep_mine_failed_auth" => {
                self.status = "Reconnect needed. Nothing was changed.".to_string();
                ctx.say(&format!("{AUTH_HELP} Nothing was changed."));
            }
            rejected if rejected.starts_with("keep_mine_failed_rejected:") => {
                // The reason code rides along in the outcome tag (see
                // start_keep_mine) so this speaks the same career-named,
                // family-split story the background auto-backup queue speaks
                // for the same reason code, instead of one fixed unnamed line
                // for every cause -- this menu is the exact button a conflicted
                // tester presses (Shane's report, 2026-08-14).
                let reason = rejected
                    .split_once(':')
                    .map(|(_, reason)| reason)
                    .unwrap_or("");
                let message = rejection_status(&self.save_name, Some(reason));
                self.status = format!(
                    "{}: backup not accepted. Nothing was changed.",
                    self.save_name
                );
                ctx.say(&format!("{message} Nothing was changed."));
            }
            "keep_mine_failed_conflict" => {
                self.status = "The cloud copy changed again. Nothing was changed.".to_string();
                ctx.say(
                    "The cloud copy changed again since this conflict was recorded. Nothing \
                     was changed. Open this career again for the current conflict.",
                );
            }
            "unverified" => {
                self.status =
                    "orinks.net has not confirmed this backup. Local save unchanged.".to_string();
                ctx.say(
                    "orinks.net has not confirmed this backup. It was not restored, and your local career is unchanged.",
                );
            }
            "integrity_failed" => {
                self.status = "This backup is damaged. Local save unchanged.".to_string();
                ctx.say(
                    "This backup is damaged. It was not restored, and your local career is unchanged.",
                );
            }
            "update_required" => {
                self.status =
                    "Backup needs a newer Freight Fate version. Nothing restored.".to_string();
                ctx.say(
                    "This backup needs a newer Freight Fate version. Update the game and try again. Nothing was restored.",
                );
            }
            "legacy_refused" => {
                self.status =
                    "Backup is from an earlier version. Nothing was restored.".to_string();
                ctx.say(&format!(
                    "{LEGACY_BACKUP_NOTICE} Your local save was not touched."
                ));
            }
            "auth_failed" => {
                self.status = "Reconnect needed. Nothing was restored.".to_string();
                ctx.say(&format!(
                    "{AUTH_HELP} Nothing was restored, and your \
                     local career is unchanged."
                ));
            }
            "invalid_profile" | "restore_failed" => {
                self.status = "Verified backup could not be saved. Nothing restored.".to_string();
                ctx.say(
                    "The verified backup could not be saved. Nothing was restored, and your local career is unchanged.",
                );
            }
            _ => {
                self.status = "Backup download failed. Local save unchanged.".to_string();
                ctx.say("The backup could not be downloaded. Your local save was not touched.");
            }
        }
    }
}

impl Menu for CloudSlotState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> =
            vec![
                MenuItem::new(format!("Status: {}", self.status), |s: &mut Self, ctx| {
                    s.speak_current(ctx)
                })
                .help("The latest restore or conflict result."),
            ];
        match self.conflict(ctx) {
            Some(conflict) => {
                items.push(
                    MenuItem::new(self.conflict_label(&conflict), |s: &mut Self, ctx| {
                        s.speak_current(ctx)
                    })
                    .help(
                        "Backups stopped because the cloud copy changed on another computer. \
                         Nothing changes until you choose.",
                    ),
                );
                let mine = self.local_summary();
                let theirs = conflict
                    .get("latestSummary")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty());
                items.push(
                    MenuItem::new(
                        // Each row names what it KEEPS. Arrowing between two rows
                        // that differ only in the words "this computer" and "the
                        // cloud" is not a choice a player can answer; arrowing
                        // between two careers with a level and a balance is.
                        format!(
                            "Keep this computer's save and back it up{}",
                            if mine.is_empty() {
                                String::new()
                            } else {
                                format!(": {mine}")
                            }
                        ),
                        |s: &mut Self, ctx| s.confirm_keep_mine(ctx),
                    )
                    .help(
                        "Uploads this computer's save over the cloud copy \
                         and turns backups for this career back on.",
                    ),
                );
                items.push(
                    MenuItem::new(
                        format!(
                            "Use the cloud copy on this computer{}",
                            theirs.map(|t| format!(": {t}")).unwrap_or_default()
                        ),
                        |s: &mut Self, ctx| {
                            let latest = s.revisions.first().cloned();
                            s.confirm_restore(ctx, latest.as_ref());
                        },
                    )
                    .help(
                        "Downloads the cloud copy over this computer's \
                         save. The current local save is kept as a fallback \
                         file beside it.",
                    ),
                );
            }
            None => {
                if self.has_local_save() {
                    // First action on the screen, present whenever there is
                    // a save here to send: the one row that can never cost
                    // the career, so it is the one a worried player reaches
                    // first. Under a conflict the two rows above replace it,
                    // because then the upload IS a choice between copies.
                    items.push(
                        MenuItem::new("Back up this career now", |s: &mut Self, ctx| {
                            s.start_backup_now(ctx)
                        })
                        .help(
                            "Sends this computer's save to your orinks.net account now and \
                             says the result. Nothing on this computer changes.",
                        ),
                    );
                }
                items.push(
                    MenuItem::new(
                        Label::dynamic(|s: &Self, _| s.restore_label()),
                        |s: &mut Self, ctx| {
                            let latest = s.revisions.first().cloned();
                            s.confirm_restore(ctx, latest.as_ref());
                        },
                    )
                    .help(
                        "Replaces this career's local save with the cloud \
                         backup. The current local save is kept as a fallback \
                         file beside it.",
                    ),
                );
                for entry in self.revisions.clone().into_iter().skip(1) {
                    let mut label = format!(
                        "Restore an older backup: {}",
                        backed_up_text(created_at_ms(&entry))
                    );
                    if let Some(summary) = summary_of(&entry) {
                        label.push_str(&format!(". {summary}"));
                    }
                    if is_legacy_snapshot(&entry) {
                        label.push_str(". From an earlier version of Freight Fate");
                    }
                    items.push(
                        MenuItem::new(label, move |s: &mut Self, ctx| {
                            s.confirm_restore(ctx, Some(&entry))
                        })
                        .help("Replaces the local save with this older backup."),
                    );
                }
            }
        }
        if !self.revisions.is_empty() {
            if self.is_public {
                items.push(
                    MenuItem::new("This is your public career", |s: &mut Self, ctx| {
                        s.speak_current(ctx)
                    })
                    .help(
                        "When Profile sharing is on, approved facts from \
                             this career's accepted backups appear on your public \
                             profile. Your other careers stay private cloud \
                             backups.",
                    ),
                );
            } else {
                items.push(
                    MenuItem::new("Make this your public career", |s: &mut Self, ctx| {
                        s.confirm_public(ctx)
                    })
                    .help(
                        "Your public profile shows one career. The others stay private cloud \
                         backups.",
                    ),
                );
            }
            items.push(
                MenuItem::new("Delete this career's cloud backups", |s: &mut Self, ctx| {
                    s.confirm_delete(ctx)
                })
                .help(
                    "Removes every kept cloud backup of this career from \
                     your orinks.net account. The save on this computer is \
                     not touched.",
                ),
            );
        }
        items.push(MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)));
        items
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
        if let Some((name, token, remaining)) = self.backup_watch.clone() {
            let outcome = match ctx.cloud_saves_service().outcome_for(&name, token) {
                Some(outcome) => outcome,
                None => {
                    let remaining = remaining - dt;
                    if remaining > 0.0 {
                        self.backup_watch = Some((name, token, remaining));
                        return;
                    }
                    // Still in flight after the bounded wait: the queue keeps
                    // retrying on its own, and the player is told so once.
                    "network".to_string()
                }
            };
            self.backup_watch = None;
            self.busy = false;
            self.speak_backup_outcome(ctx, &outcome);
            self.refresh(ctx, false);
            return;
        }
        let Some(outcome) = self.outcome.take() else {
            return;
        };
        self.busy = false;
        if outcome == "restored" {
            if let Some(path) = self.restored.take() {
                self.restored_path = Some(path);
            }
        }
        self.speak_outcome(ctx, &outcome);
        self.refresh(ctx, false);
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        if self.busy {
            ctx.say("Cloud backup is still working. Stay here for the result.");
            return;
        }
        menu_default_go_back(self, ctx);
    }
}

impl_state_for_menu!(CloudSlotState);
