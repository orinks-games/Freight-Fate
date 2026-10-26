//! The one-time offer to connect this computer to an orinks.net account
//! (port of `freight_fate/states/online_offer.py`).
//!
//! Shown once, straight after a first career is created, because nothing else
//! tells a new player the feature exists. Online is optional and stays optional:
//! declining takes one keypress, sets the gate, and is never asked again.
//!
//! The copy says exactly what connecting does: it turns cloud backup and Profile
//! sharing on. It does, since 1.9 -- a connected account that publishes nothing
//! and backs nothing up is a connection that did nothing for the player, and the
//! career statistics on the public profile are read from the backup. The rule
//! behind the wording is unchanged though: a player must never walk away from
//! this prompt with a wrong idea of what is backed up or public, in either
//! direction.

use crate::app::{GameContext, Say};
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::city::CityMenuState;
use crate::states::online_states::{load_identity, OnlineSetupState};

/// Whether a first-run player should hear the offer at all.
pub fn should_offer_online(ctx: &GameContext) -> bool {
    if ctx.settings.online_offer_seen {
        return false;
    }
    load_identity().is_none()
}

pub struct OnlineOfferState {
    pub menu: MenuCore<Self>,
}

impl Default for OnlineOfferState {
    fn default() -> Self {
        Self {
            menu: MenuCore::new(Self::TITLE).with_intro_help(Self::INTRO_HELP),
        }
    }
}

impl OnlineOfferState {
    pub const TITLE: &'static str = "Connect to orinks.net";
    pub const INTRO_HELP: &'static str =
        "Choose Set up now to connect this computer, or Not now to start driving.";

    /// `OnlineOfferState(ctx)`.
    pub fn new(_ctx: &mut GameContext) -> Self {
        Self::default()
    }

    fn spend_the_offer(&mut self, ctx: &mut GameContext) {
        ctx.settings.online_offer_seen = true;
        if let Err(e) = ctx.settings.save() {
            log::warn!("Could not save settings: {e}");
        }
    }

    fn enter_world(&mut self, ctx: &mut GameContext) {
        // The city menu queues its own announcement, so a line spoken on the
        // way out of here -- "Online on the main menu connects any time" -- is
        // heard in full instead of being cut off by the terminal's own line.
        let city = CityMenuState::new(ctx, true);
        ctx.replace_state(city);
    }

    /// `_decline`: Not now.
    pub fn decline(&mut self, ctx: &mut GameContext) {
        self.spend_the_offer(ctx);
        ctx.say("Online on the main menu connects any time.");
        self.enter_world(ctx);
    }

    /// `_accept`: Set up now.
    pub fn accept(&mut self, ctx: &mut GameContext) {
        // The player already said "Set up now" -- pushing OnlineSetupState
        // with autostart=True starts activation immediately instead of
        // asking them to confirm the same decision again from a menu. The
        // city menu goes underneath (replace, not push) so that backing out
        // of setup lands the player in the world, not back on this offer.
        self.spend_the_offer(ctx);
        self.enter_world(ctx);
        let setup = OnlineSetupState::with_autostart(ctx, true);
        ctx.push_state(setup);
    }
}

impl Menu for OnlineOfferState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        // Queued, not interrupting: career creation speaks the welcome line
        // immediately before pushing this state, and the player has to hear
        // where they are and what they own before being asked anything.
        let current = self.current_text(ctx);
        ctx.say_with(
            format!(
                "Before you set off. You can connect this computer to an \
                 orinks.net account. That backs your career up so you can bring \
                 it to another computer, and puts your driver profile and on-duty \
                 activity on the public site. You can turn either of those off \
                 afterwards from Online on the main menu. It takes a code and \
                 your browser, and you can do it any time instead. \
                 {current}"
            ),
            Say::queued(),
        );
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        // Not now first, so the cursor starts on the answer that changes
        // nothing. Escape takes the same path.
        vec![
            MenuItem::new("Not now", |s: &mut Self, ctx| s.decline(ctx))
                .help("Start driving. You can connect later from Online."),
            MenuItem::new("Set up now", |s: &mut Self, ctx| s.accept(ctx)).help(
                "Connect this computer to an orinks.net account, which turns on \
                 cloud backup and your public driver profile.",
            ),
        ]
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        // Escape means Not now. The player must never be stuck here, and
        // backing out still spends the offer so it cannot reappear.
        self.decline(ctx);
    }
}

impl_state_for_menu!(OnlineOfferState);
