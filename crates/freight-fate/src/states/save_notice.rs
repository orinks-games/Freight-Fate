//! One-time spoken notices about a save file: converted, or changed outside
//! the game (port of `freight_fate/states/save_notice.py`).

use ff_core::models::enforcement;

use crate::app::GameContext;
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::main_menu::{pending_notice_state, world_entry_state, NameEntryState};

/// Replace this notice with the next pending one, or the world.
fn continue_to_career(ctx: &mut GameContext) {
    let next = pending_notice_state(ctx).unwrap_or_else(|| world_entry_state(ctx, false));
    ctx.replace_shared_with(next, true, true);
}

pub struct SaveModifiedNoticeState {
    menu: MenuCore<Self>,
}

impl SaveModifiedNoticeState {
    pub fn new() -> Self {
        Self {
            menu: MenuCore::new("Save file changed outside the game")
                .with_intro_help("Enter continues to your career."),
        }
    }

    fn acknowledge(&mut self, ctx: &mut GameContext) {
        if let Some(p) = ctx.profile.as_mut() {
            p.integrity_notice_pending = false;
            if let Err(e) = p.save() {
                log::error!("Could not save the profile: {e}");
            }
        }
        continue_to_career(ctx);
    }
}

impl Default for SaveModifiedNoticeState {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu for SaveModifiedNoticeState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let text = format!(
            "This save was changed outside the game, or copied from another \
             computer, so it is marked as modified. Your career still works on \
             this computer, but profile sharing may not accept a modified \
             profile. {}",
            self.current_text(ctx)
        );
        ctx.say(&text);
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![MenuItem::new("OK", |s: &mut Self, ctx| s.acknowledge(ctx))
            .help("Continue to your career.")]
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        // Escape acknowledges too; the player must never be stuck here.
        self.acknowledge(ctx);
    }
}

impl_state_for_menu!(SaveModifiedNoticeState);

/// A career from before 1.9 was picked from the list: explain, offer a
/// fresh start, and leave the old save exactly as it is.
///
/// Unlike the two notices above, no profile is loaded here -- the load gate
/// refused the file without touching it, and this state only knows the
/// driver name it carried.
pub struct LegacyCareerNoticeState {
    menu: MenuCore<Self>,
    pub driver_name: String,
}

impl LegacyCareerNoticeState {
    pub fn new(driver_name: &str) -> Self {
        Self {
            menu: MenuCore::new("Career from an earlier version").with_intro_help(
                "This career cannot continue in version 1.9. Escape goes back to \
                 the career list.",
            ),
            driver_name: driver_name.to_string(),
        }
    }

    fn new_career(&mut self, ctx: &mut GameContext) {
        ctx.replace_state(NameEntryState::new());
    }
}

impl Menu for LegacyCareerNoticeState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let text = format!(
            "{} was made in an earlier version of Freight Fate. Version 1.9 \
             rebalances the whole career, from pay to trucks to levels, so \
             every driver starts fresh. Nothing was lost: the save is still \
             on this computer, untouched, and it still works in Freight Fate \
             1.8. {}",
            self.driver_name,
            self.current_text(ctx)
        );
        ctx.say(&text);
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![
            MenuItem::new("Start a new career", |s: &mut Self, ctx| s.new_career(ctx))
                .help("A fresh 1.9 career with a new driver name."),
            MenuItem::new("Back to the career list", |s: &mut Self, ctx| {
                s.go_back(ctx)
            })
            .help("Back to the saved careers, nothing changed."),
        ]
    }
}

impl_state_for_menu!(LegacyCareerNoticeState);

/// A career that predates the enforcement record, told where it stands.
///
/// There is no amnesty here: every offense the save still holds counts, and
/// a reputation dispatch has already lost faith in counts too. The one thing
/// owed is a plain explanation, once, before the player finds out by way of
/// a short board they cannot account for.
pub struct DrivingRecordNoticeState {
    menu: MenuCore<Self>,
}

impl DrivingRecordNoticeState {
    pub fn new() -> Self {
        Self {
            menu: MenuCore::new("Your driving record")
                .with_intro_help("Enter continues to your career."),
        }
    }

    fn acknowledge(&mut self, ctx: &mut GameContext) {
        if let Some(p) = ctx.profile.as_mut() {
            p.driving_record.notice_pending = false;
            if let Err(e) = p.save() {
                log::error!("Could not save the profile: {e}");
            }
        }
        continue_to_career(ctx);
    }
}

impl Default for DrivingRecordNoticeState {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu for DrivingRecordNoticeState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let (standing, trust) = match ctx.profile.as_ref() {
            Some(p) => (
                enforcement::standing_text(p),
                enforcement::trust_text_with_display(p.standing(), ctx.settings.score_display),
            ),
            None => (String::new(), String::new()),
        };
        let text = format!(
            "Freight Fate now keeps a driving record for your career: \
             citations, serious violations, and whether your CDL is clear. \
             Two serious violations in three years suspend it; running from a \
             police stop is a major offense that disqualifies it for a year. \
             Reputation also decides how much freight dispatch shows you and \
             how much choice you get. Nothing was reset or taken. {standing} \
             {trust} {}",
            self.current_text(ctx)
        );
        ctx.say(&text);
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![MenuItem::new("OK", |s: &mut Self, ctx| s.acknowledge(ctx))
            .help("Continue to your career.")]
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        // Escape acknowledges too; the player must never be stuck here.
        self.acknowledge(ctx);
    }
}

impl_state_for_menu!(DrivingRecordNoticeState);

pub struct SaveMigrationNoticeState {
    menu: MenuCore<Self>,
}

impl SaveMigrationNoticeState {
    pub fn new() -> Self {
        Self {
            menu: MenuCore::new("Save file updated")
                .with_intro_help("Enter continues to your career."),
        }
    }

    fn acknowledge(&mut self, ctx: &mut GameContext) {
        if let Some(p) = ctx.profile.as_mut() {
            p.migration_notice_pending = false;
            if let Err(e) = p.save() {
                log::error!("Could not save the profile: {e}");
            }
        }
        continue_to_career(ctx);
    }
}

impl Default for SaveMigrationNoticeState {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu for SaveMigrationNoticeState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let text = format!(
            "Save file updated. This career was converted from an older \
             version: every truck you own now keeps its own fuel, damage, tire \
             wear, and road grime. The truck you were driving keeps its \
             condition; your other trucks start fueled and fresh. The updated \
             save no longer opens in older versions of the game. {}",
            self.current_text(ctx)
        );
        ctx.say(&text);
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![MenuItem::new("OK", |s: &mut Self, ctx| s.acknowledge(ctx))
            .help("Continue to your career.")]
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        // Escape acknowledges too; the player must never be stuck here.
        self.acknowledge(ctx);
    }
}

impl_state_for_menu!(SaveMigrationNoticeState);
