//! The Cloud backup menu: restore cloud saves and resolve sync conflicts
//! (port of `freight_fate/states/cloud_save_states.py`).
//!
//! Reached from the Online menu. Everything network runs on daemon threads
//! with the same mailbox-polling pattern as the drivers board menus: the game
//! loop and speech stay responsive while orinks.net answers.

use crate::states::base::default_enter;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ff_core::models::profile::{
    encode_save_bytes, find_save_path, save_path_for, Profile, FIRST_1_9_SAVE_VERSION,
};
use ff_core::pyfmt::round_py_int;
use serde_json::Value;

use crate::app::GameContext;
use crate::cloud_saves::{self, backup_summary, save_slot_name, SavesList, AUTH_HELP};
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::online_states::{load_identity, run_worker, wall_time, Mailbox};

mod slot;

pub use slot::{CloudSlotState, SlotHandle};

// Spoken when a backup made before the 1.9 line is picked for restore: the
// same fresh-start rule as the local load gate, said kindly, with the backup
// left exactly where it is.
pub const LEGACY_BACKUP_NOTICE: &str =
    "This backup was made by an earlier version of Freight Fate. Version \
1.9 rebalances the whole career, so every driver starts fresh, and \
earlier careers cannot be restored here. The backup stays safe in \
your orinks.net account, and Freight Fate 1.8 can still restore it.";

/// Whether a cloud revision's metadata says it predates the 1.9 line.
///
/// The server records the save version of every upload (the validator reads
/// it first), so the list can label old backups without downloading them.
/// Metadata-free entries pass; the restore path checks the downloaded
/// profile itself either way.
pub fn is_legacy_snapshot(entry: &Value) -> bool {
    match entry.get("saveVersion") {
        Some(Value::Number(n)) if n.is_i64() || n.is_u64() => {
            let version = n.as_i64().unwrap_or(0);
            0 < version && version < FIRST_1_9_SAVE_VERSION
        }
        _ => false,
    }
}

pub const CLOUD_DISCLOSURE: &str =
    "Your full career is stored privately in your orinks.net account. orinks.net \
validates and signs accepted backups before they can be restored. If \
Profile sharing is also on, approved facts from your public career's \
latest accepted backup may appear in your public profile. You choose \
your public career here. Every other career stays a private cloud \
backup. The backup itself is never public. The last ten accepted \
backups of each career are kept.";

/// This computer's copy of `save_name`, in the same words the cloud
/// copy is described with, or "" if it cannot be read.
///
/// Module level because both the career list and the single-career screen
/// need it, and neither should be the one that owns it. Never fails: a
/// save that will not load costs a sentence, never the screen that resolves
/// the conflict.
pub fn local_summary_for(save_name: &str) -> String {
    let Some(path) = find_save_path(save_name) else {
        return String::new();
    };
    match Profile::load(&path) {
        Ok(profile) => backup_summary(&Value::Object(profile.to_dict())),
        Err(_) => String::new(),
    }
}

/// `entry.get("createdAt")` as a number, `None` when missing or not one.
pub fn created_at_ms(entry: &Value) -> Option<f64> {
    match entry.get("createdAt") {
        Some(Value::Number(n)) => n.as_f64(),
        _ => None,
    }
}

/// `entry.get("summary")` when it is a non-empty string.
pub(crate) fn summary_of(entry: &Value) -> Option<&str> {
    entry
        .get("summary")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

/// `entry.get("saveName")` as text, "" when missing.
pub(crate) fn save_name_of(entry: &Value) -> &str {
    entry.get("saveName").and_then(Value::as_str).unwrap_or("")
}

/// A speakable freshness phrase from a server epoch-milliseconds stamp.
pub fn backed_up_text(created_at_ms: Option<f64>) -> String {
    let Some(created_at_ms) = created_at_ms.filter(|ms| *ms != 0.0) else {
        return "backup time unknown".to_string();
    };
    let age_s = (wall_time() - created_at_ms / 1000.0).max(0.0);
    if age_s < 90.0 {
        return "backed up just now".to_string();
    }
    if age_s < 90.0 * 60.0 {
        return format!("backed up {} minutes ago", round_py_int(age_s / 60.0));
    }
    if age_s < 36.0 * 3600.0 {
        return format!("backed up {} hours ago", round_py_int(age_s / 3600.0));
    }
    format!("backed up {} days ago", round_py_int(age_s / 86400.0))
}

/// The profile-side half of `cloud_saves.restore_to_disk`: build the
/// `Profile` from the verified dict, sign it for this installation, and
/// atomically install it over the local file, keeping the old file beside
/// it as `.ffsave.bak` and putting it back if installation fails. A
/// leftover plain-JSON save for the career is moved aside so only one live
/// copy remains.
// It stays in the game crate because the slot name is
// `cloud_saves::save_slot_name`.
pub fn install_restored_profile(profile: &Value) -> Result<PathBuf, String> {
    let Value::Object(map) = profile else {
        return Err("cloud save content is not a profile object".to_string());
    };
    let profile = Profile::from_dict(map);
    let signed = profile.to_dict();
    let name = save_slot_name(&profile.name);
    let path = save_path_for(&name);
    ff_core::models::profile::sealed::ensure_writable(&path).map_err(|e| e.to_string())?;
    let backup = path.with_extension("ffsave.bak");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let mut moved_old = false;
    let installed = (|| -> std::io::Result<()> {
        if path.exists() {
            let _ = fs::remove_file(&backup);
            fs::rename(&path, &backup)?;
            moved_old = true;
        }
        ff_core::models::profile::sealed::write(&path, &encode_save_bytes(&signed))
    })();
    if let Err(e) = installed {
        if moved_old && backup.exists() && !path.exists() {
            let _ = fs::rename(&backup, &path);
        }
        return Err(e.to_string());
    }
    // A leftover plain-JSON save for this career would shadow nothing (the
    // packed file wins), but move it aside so only one live copy remains.
    let legacy = path.with_extension("json");
    if legacy.exists() {
        let _ = fs::rename(&legacy, legacy.with_extension("json.bak"));
    }
    Ok(path)
}

// -- CloudBackupState ---------------------------------------------------------------------------

/// What the list worker posts back.
#[derive(Debug, Clone, PartialEq)]
pub enum ListOutcome {
    AuthFailed,
    Unreachable,
    Saves(SavesList),
}

/// Cloud slots as a spoken list: one item per career, newest first.
///
/// Entering fetches the slot list on a worker thread. A slot with a sync
/// conflict says so in its label; selecting any slot opens its actions.
pub struct CloudBackupState {
    pub menu: MenuCore<Self>,
    pub saves: Option<Vec<Value>>,
    pub public_save: Option<String>,
    pub auth_failed: bool,
    fetched: Arc<AtomicBool>,
    result: Mailbox<ListOutcome>,
    announced: bool,
    pub status: String,
    pub threaded: bool,
}

impl CloudBackupState {
    pub const TITLE: &'static str = "Cloud backup";

    /// `CloudBackupState(ctx)`.
    pub fn new(_ctx: &mut GameContext) -> Self {
        Self {
            menu: MenuCore::new(Self::TITLE),
            saves: None,
            public_save: None,
            auth_failed: false,
            fetched: Arc::new(AtomicBool::new(false)),
            result: Mailbox::new(),
            announced: false,
            status: "Checking your cloud backups.".to_string(),
            threaded: true,
        }
    }

    /// Whether the list fetch has answered (`self._fetched.is_set()`).
    pub fn fetched(&self) -> bool {
        self.fetched.load(Ordering::SeqCst)
    }

    fn start_fetch(&mut self, ctx: &GameContext) {
        self.saves = None;
        self.auth_failed = false;
        self.fetched = Arc::new(AtomicBool::new(false));
        self.result = Mailbox::new();
        self.announced = false;
        let Some(identity) = load_identity() else {
            self.fetched.store(true, Ordering::SeqCst);
            return;
        };
        let fetched = Arc::clone(&self.fetched);
        let result = self.result.clone();
        let transport = ctx.cloud_saves_service().transport().clone();
        run_worker(self.threaded, "cloud-saves-list", move || {
            let outcome = match cloud_saves::list_saves(&identity, transport.as_ref()) {
                Err(cloud_saves::CloudAuthError) => ListOutcome::AuthFailed,
                Ok(None) => ListOutcome::Unreachable,
                Ok(Some(list)) => ListOutcome::Saves(list),
            };
            result.post(outcome);
            fetched.store(true, Ordering::SeqCst);
        });
    }

    /// Move a landed fetch out of the mailbox (the worker set `_saves` and
    /// `_auth_failed` directly; here they travel in the mailbox).
    fn absorb(&mut self) {
        if !self.fetched() {
            return;
        }
        match self.result.take() {
            Some(ListOutcome::AuthFailed) => self.auth_failed = true,
            Some(ListOutcome::Unreachable) | None => {}
            Some(ListOutcome::Saves(list)) => {
                self.saves = Some(list.saves);
                self.public_save = list.public_save_name;
            }
        }
    }

    /// Latest revision per slot, in the server's newest-first order.
    pub fn slots(&self) -> Vec<Value> {
        let mut seen: Vec<&str> = Vec::new();
        let mut out = Vec::new();
        for entry in self.saves.iter().flatten() {
            if let Some(name) = entry.get("saveName").and_then(Value::as_str) {
                if !seen.contains(&name) {
                    seen.push(name);
                    out.push(entry.clone());
                }
            }
        }
        out
    }

    fn speak_disclosure(&mut self, ctx: &mut GameContext) {
        ctx.say(CLOUD_DISCLOSURE);
    }

    fn turn_on(&mut self, ctx: &mut GameContext) {
        let consent = CloudBackupConsentState::new(ctx);
        ctx.push_state(consent);
    }

    fn refresh_list(&mut self, ctx: &mut GameContext) {
        self.status = "Checking your cloud backups.".to_string();
        self.start_fetch(ctx);
        self.refresh(ctx, false);
        ctx.say("Checking your cloud backups.");
    }

    fn open_slot(&mut self, ctx: &mut GameContext, entry: &Value) {
        let name = save_name_of(entry).to_string();
        let revisions: Vec<Value> = self
            .saves
            .iter()
            .flatten()
            .filter(|e| save_name_of(e) == name)
            .cloned()
            .collect();
        // This list screen is the one the slot reports its public-career
        // choice back to; it is the stack's top while its own handler runs.
        let parent = ctx.state();
        let public = self.public_save.clone();
        let slot = CloudSlotState::new(ctx, &name, revisions, public.as_deref(), parent);
        ctx.push_state(slot);
    }

    /// Keep the list truthful after the slot menu changes the choice,
    /// without another fetch.
    pub fn public_career_chosen(&mut self, ctx: &mut GameContext, save_name: &str) {
        self.public_save = Some(save_name.to_string());
        self.refresh(ctx, true);
    }
}

impl Menu for CloudBackupState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn enter(&mut self, ctx: &mut GameContext) {
        self.start_fetch(ctx);
        default_enter(self, ctx);
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        if load_identity().is_none() {
            return vec![
                MenuItem::new(
                    "Cloud backup needs your orinks.net driver account",
                    |s: &mut Self, ctx| s.speak_current(ctx),
                )
                .help("Set up your orinks.net account on the Online menu first."),
                MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)),
            ];
        }
        self.absorb();
        if !self.fetched() {
            return vec![
                MenuItem::new("Checking your cloud backups", |s: &mut Self, ctx| {
                    s.speak_current(ctx)
                }),
                MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)),
            ];
        }
        let service = ctx.cloud_saves_service();
        if self.auth_failed {
            self.status = "Reconnect needed: orinks.net no longer accepts this computer's sign-in."
                .to_string();
        } else if self.saves.is_none() {
            self.status = "Cloud backups could not be reached.".to_string();
        } else {
            self.status = service.status();
        }
        let mut items: Vec<MenuItem<Self>> =
            vec![
                MenuItem::new(format!("Status: {}", self.status), |s: &mut Self, ctx| {
                    s.speak_current(ctx)
                })
                .help("The latest Cloud backup result."),
            ];
        if !service.enabled() {
            items.push(
                MenuItem::new("Turn Cloud backup on", |s: &mut Self, ctx| s.turn_on(ctx))
                    .help("Explains cloud backup, then asks."),
            );
        }
        let slots = self.slots();
        if self.auth_failed {
            items.push(
                MenuItem::new(
                    "Reconnect needed: your orinks.net sign-in is no longer accepted",
                    |s: &mut Self, ctx| s.speak_current(ctx),
                )
                .help(AUTH_HELP),
            );
        } else if self.saves.is_none() {
            items.push(
                MenuItem::new(
                    "Your cloud backups could not be reached",
                    |s: &mut Self, ctx| s.speak_current(ctx),
                )
                .help("orinks.net did not answer. Refresh to try again."),
            );
        } else if slots.is_empty() {
            items.push(
                MenuItem::new("No cloud backups yet", |s: &mut Self, ctx| {
                    s.speak_current(ctx)
                })
                .help("Backups appear here after the game saves with cloud backup turned on."),
            );
        } else {
            let conflicts = service.conflicts();
            for entry in slots {
                let name = save_name_of(&entry).to_string();
                let mut bits = vec![name.clone()];
                if self.public_save.as_deref() == Some(name.as_str()) {
                    bits.push("your public career".to_string());
                }
                if let Some(summary) = summary_of(&entry) {
                    bits.push(summary.to_string());
                }
                bits.push(backed_up_text(created_at_ms(&entry)));
                if is_legacy_snapshot(&entry) {
                    bits.push("from an earlier version of Freight Fate".to_string());
                }
                if conflicts.contains_key(&name) {
                    // The summary already read out above is the CLOUD copy
                    // (it comes from the server row), so the thing still
                    // missing here is what this computer holds. Naming it
                    // makes the difference audible from the list, before the
                    // player has to open anything to find out what is at
                    // stake.
                    bits.push("needs attention: this computer has a different copy".to_string());
                    let mine = local_summary_for(&name);
                    if !mine.is_empty() {
                        bits.push(format!("this computer's copy is {mine}"));
                    }
                    bits.push("Open this career to choose which copy to keep".to_string());
                }
                items.push(
                    MenuItem::new(bits.join(". "), move |s: &mut Self, ctx| {
                        s.open_slot(ctx, &entry)
                    })
                    .help("Enter opens restore choices for this career."),
                );
            }
        }
        items.push(MenuItem::new(
            "Hear how cloud backup works",
            |s: &mut Self, ctx| s.speak_disclosure(ctx),
        ));
        items.push(
            MenuItem::new("Refresh", |s: &mut Self, ctx| s.refresh_list(ctx))
                .help("Check the backups again."),
        );
        items.push(MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)));
        items
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
        if self.announced || !self.fetched() || load_identity().is_none() {
            return;
        }
        self.announced = true;
        self.absorb();
        if self.auth_failed {
            self.status = "Reconnect needed: orinks.net no longer accepts this computer's sign-in."
                .to_string();
            self.refresh(ctx, false);
            ctx.say(AUTH_HELP);
            return;
        }
        if self.saves.is_none() {
            self.status = "Cloud backups could not be reached.".to_string();
            self.refresh(ctx, false);
            ctx.say("Your cloud backups could not be reached.");
            return;
        }
        let slots = self.slots();
        self.status = ctx.cloud_saves_service().status();
        self.refresh(ctx, false);
        if slots.is_empty() {
            ctx.say("No cloud backups yet.");
        } else {
            let count = format!(
                "{} career{}",
                slots.len(),
                if slots.len() != 1 { "s are" } else { " is" }
            );
            let current = self.current_text(ctx);
            ctx.say(&format!("{count} backed up. {current}"));
        }
    }
}

impl_state_for_menu!(CloudBackupState);

// -- ConfirmRestoreState ------------------------------------------------------------------------

/// One spoken yes/no gate before a restore overwrites a local save.
pub struct ConfirmRestoreState {
    pub menu: MenuCore<Self>,
    slot: SlotHandle,
    entry: Value,
}

impl ConfirmRestoreState {
    pub const TITLE: &'static str = "Restore this backup?";

    /// `ConfirmRestoreState(ctx, slot_state, entry)`.
    pub fn new(_ctx: &mut GameContext, slot: SlotHandle, entry: Value) -> Self {
        Self {
            menu: MenuCore::new(Self::TITLE),
            slot,
            entry,
        }
    }

    fn yes(&mut self, ctx: &mut GameContext) {
        ctx.pop_state();
        let entry = self.entry.clone();
        self.slot
            .with(ctx, |slot, ctx| slot.start_restore(ctx, &entry));
    }
}

impl Menu for ConfirmRestoreState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let summary = match summary_of(&self.entry) {
            Some(summary) => format!(" It is {summary},"),
            None => String::new(),
        };
        let current = self.current_text(ctx);
        ctx.say(&format!(
            "Restore the backup of {}, \
             {}?{summary} replacing \
             this computer's save for that career. The replaced save is kept \
             as a fallback file. {current}",
            self.slot.save_name,
            backed_up_text(created_at_ms(&self.entry))
        ));
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![
            MenuItem::new(
                // NOT "No, keep this computer's save". That is word for word
                // what the conflict screen's real action is called ("Keep
                // this computer's save and back it up"), so a player who
                // wants exactly that hears it here, presses it, and gets
                // nothing -- the owner did it on his own career while
                // testing, and it is the likeliest reason Brandon got no
                // further either (2026-08-15). A cancel says it cancels.
                "No, cancel and change nothing",
                |s: &mut Self, ctx| s.go_back(ctx),
            )
            .help(
                "Goes back without downloading. This computer's save stays as it is. To send \
                 this computer's save up instead, choose Keep this computer's save and back \
                 it up.",
            ),
            MenuItem::new("Yes, restore this backup", |s: &mut Self, ctx| s.yes(ctx)),
        ]
    }
}

impl_state_for_menu!(ConfirmRestoreState);

// -- CloudBackupConsentState -----------------------------------------------------------------

/// Safe-default confirmation before private full-career uploads begin.
pub struct CloudBackupConsentState {
    pub menu: MenuCore<Self>,
}

impl Default for CloudBackupConsentState {
    fn default() -> Self {
        Self {
            menu: MenuCore::new(Self::TITLE),
        }
    }
}

impl CloudBackupConsentState {
    pub const TITLE: &'static str = "Turn Cloud backup on?";

    /// `CloudBackupConsentState(ctx)`.
    pub fn new(_ctx: &mut GameContext) -> Self {
        Self::default()
    }

    fn yes(&mut self, ctx: &mut GameContext) {
        ctx.settings.cloud_saves = true;
        if let Err(e) = ctx.settings.save() {
            log::warn!("Could not save settings: {e}");
        }
        ctx.apply_cloud_saves();
        ctx.pop_state();
        ctx.say("Cloud backup is on. The next save backs up privately.");
    }
}

impl Menu for CloudBackupConsentState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let current = self.current_text(ctx);
        ctx.say(&format!("{CLOUD_DISCLOSURE} {current}"));
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![
            MenuItem::new("No, keep Cloud backup off", |s: &mut Self, ctx| {
                s.go_back(ctx)
            }),
            MenuItem::new("Yes, turn Cloud backup on", |s: &mut Self, ctx| s.yes(ctx)),
        ]
    }
}

impl_state_for_menu!(CloudBackupConsentState);

// -- ConfirmDeleteCloudState ------------------------------------------------------------------

/// Safe-default gate before removing every cloud backup of one career.
pub struct ConfirmDeleteCloudState {
    pub menu: MenuCore<Self>,
    slot: SlotHandle,
}

impl ConfirmDeleteCloudState {
    pub const TITLE: &'static str = "Delete the cloud backups?";

    /// `ConfirmDeleteCloudState(ctx, slot_state)`.
    pub fn new(_ctx: &mut GameContext, slot: SlotHandle) -> Self {
        Self {
            menu: MenuCore::new(Self::TITLE),
            slot,
        }
    }

    fn yes(&mut self, ctx: &mut GameContext) {
        ctx.pop_state();
        self.slot.with(ctx, |slot, ctx| slot.start_delete(ctx));
    }
}

impl Menu for ConfirmDeleteCloudState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let local = if find_save_path(&self.slot.save_name).is_some() {
            "Your save on this computer is not touched. Its next save starts a fresh backup."
        } else {
            "This career has no save on this computer."
        };
        let current = self.current_text(ctx);
        ctx.say(&format!(
            "Delete every cloud backup of {} \
             from your orinks.net account? The deleted backups cannot be \
             brought back. {local} {current}",
            self.slot.save_name
        ));
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![
            MenuItem::new(
                // Same rule as the restore confirmation above: a cancel row
                // must not be phrased as an outcome another control already
                // delivers, or it reads as the action rather than the retreat.
                "No, cancel and change nothing",
                |s: &mut Self, ctx| s.go_back(ctx),
            )
            .help("Goes back without deleting anything."),
            MenuItem::new(
                "Yes, delete every cloud backup of this career",
                |s: &mut Self, ctx| s.yes(ctx),
            ),
        ]
    }
}

impl_state_for_menu!(ConfirmDeleteCloudState);

// -- ConfirmPublicCareerState -----------------------------------------------------------------

/// Safe-default gate before switching which career fronts the profile.
pub struct ConfirmPublicCareerState {
    pub menu: MenuCore<Self>,
    slot: SlotHandle,
}

impl ConfirmPublicCareerState {
    pub const TITLE: &'static str = "Make this your public career?";

    /// `ConfirmPublicCareerState(ctx, slot_state)`.
    pub fn new(_ctx: &mut GameContext, slot: SlotHandle) -> Self {
        Self {
            menu: MenuCore::new(Self::TITLE),
            slot,
        }
    }

    fn yes(&mut self, ctx: &mut GameContext) {
        ctx.pop_state();
        self.slot.with(ctx, |slot, ctx| slot.start_set_public(ctx));
    }
}

impl Menu for ConfirmPublicCareerState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let current = self.current_text(ctx);
        ctx.say(&format!(
            "Make {} your public career? Your public profile shows one career, and the \
             others stay private cloud backups. {current}",
            self.slot.save_name
        ));
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![
            MenuItem::new("No, keep things as they are", |s: &mut Self, ctx| {
                s.go_back(ctx)
            }),
            MenuItem::new("Yes, make this my public career", |s: &mut Self, ctx| {
                s.yes(ctx)
            }),
        ]
    }
}

impl_state_for_menu!(ConfirmPublicCareerState);

// -- ConfirmKeepMineState -----------------------------------------------------------------------

/// Safe-default gate before overwriting the accepted cloud copy.
pub struct ConfirmKeepMineState {
    pub menu: MenuCore<Self>,
    slot: SlotHandle,
}

impl ConfirmKeepMineState {
    pub const TITLE: &'static str = "Replace the cloud backup?";

    /// `ConfirmKeepMineState(ctx, slot_state)`.
    pub fn new(_ctx: &mut GameContext, slot: SlotHandle) -> Self {
        Self {
            menu: MenuCore::new(Self::TITLE),
            slot,
        }
    }

    fn yes(&mut self, ctx: &mut GameContext) {
        ctx.pop_state();
        self.slot.with(ctx, |slot, ctx| slot.start_keep_mine(ctx));
    }
}

impl Menu for ConfirmKeepMineState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let current = self.current_text(ctx);
        ctx.say(&format!(
            "Replace the accepted cloud copy with this computer's save? \
             The server will validate it first. {current}"
        ));
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![
            MenuItem::new(
                // Same trap in the other direction: phrased as an outcome,
                // "keep the current cloud backup" reads like the choice to
                // take the cloud copy, on the one screen where picking the
                // wrong one of two similar-sounding rows leaves a career
                // stuck exactly as it was.
                "No, cancel and change nothing",
                |s: &mut Self, ctx| s.go_back(ctx),
            )
            .help(
                "Goes back without uploading. This career does not back up until you choose \
                 which copy to keep.",
            ),
            MenuItem::new(
                "Yes, validate and replace the cloud backup",
                |s: &mut Self, ctx| s.yes(ctx),
            ),
        ]
    }
}

impl_state_for_menu!(ConfirmKeepMineState);
