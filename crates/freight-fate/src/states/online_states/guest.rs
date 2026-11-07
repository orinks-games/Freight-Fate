//! Going online as a guest, and asking for a new guest name (see
//! `crate::online_guest`). Both talk to orinks.net on a worker and read the
//! answer from a [`Mailbox`] in `update`, like the rest of the online menus.

use ff_core::models::profile::{data_dir, data_dir_if_allowed};

use super::{
    load_identity, menu_default_go_back, online_transport, run_worker, save_identity, Mailbox,
    PROFILE_SHARING_CONSENT_VERSION,
};
use crate::app::GameContext;
use crate::impl_state_for_menu;
use crate::online_guest::{self, GuestCreate, GuestRecord, GuestRename};
use crate::online_presence::{self, OnlineIdentity};
use crate::states::base::{Label, Menu, MenuCore, MenuItem};

pub const GUEST_DISCLOSURE: &str = "As a guest you go on the orinks.net drivers board under a \
name orinks.net picks, like Copper Fox 12, with no account. This turns Profile sharing on, so \
your guest name and on-duty activity show publicly on the board. A driver profile, road \
journal, achievements and cloud backup need an account. Profile sharing on the Online menu \
turns sharing off.";

/// The guest this computer is signed in as, or `None`. Cheap enough for a
/// menu label: one small file next to the identity.
pub fn current_guest() -> Option<GuestRecord> {
    let dir = data_dir_if_allowed()?;
    online_guest::guest_for(load_identity().as_ref(), &dir)
}

// -- GuestSetupState --------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestPhase {
    Idle,
    Creating,
    Sharing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuestOutcome {
    Created(GuestCreate),
    /// `online_presence::set_profile_sharing`'s answer.
    Sharing(&'static str),
}

/// "Drive online as a guest": ask orinks.net for a guest, save it, and turn
/// Profile sharing on.
pub struct GuestSetupState {
    pub menu: MenuCore<Self>,
    pub phase: GuestPhase,
    pub name: Option<String>,
    pub outcome: Mailbox<GuestOutcome>,
    pub threaded: bool,
}

impl GuestSetupState {
    pub const TITLE: &'static str = "Drive online as a guest";

    pub fn new(_ctx: &mut GameContext) -> Self {
        Self {
            menu: MenuCore::new(Self::TITLE),
            phase: GuestPhase::Idle,
            name: None,
            outcome: Mailbox::new(),
            threaded: true,
        }
    }

    fn start_label(&self) -> String {
        match self.phase {
            GuestPhase::Idle => "Go online as a guest".to_string(),
            GuestPhase::Creating => "Asking orinks.net for a guest name".to_string(),
            GuestPhase::Sharing => "Turning Profile sharing on".to_string(),
        }
    }

    pub fn start(&mut self, ctx: &mut GameContext) {
        if self.phase != GuestPhase::Idle {
            ctx.say("Still working. Stay here for the result.");
            return;
        }
        self.phase = GuestPhase::Creating;
        self.refresh(ctx, true);
        ctx.say("Asking orinks.net for a guest name.");
        let outcome = self.outcome.clone();
        let transport = online_transport();
        let dir = data_dir();
        run_worker(self.threaded, "online-guest", move || {
            outcome.post(GuestOutcome::Created(online_guest::create_guest(
                transport.as_ref(),
                &dir,
            )));
        });
    }

    fn finish_created(&mut self, ctx: &mut GameContext, created: GuestCreate) {
        let (identity, name) = match created {
            GuestCreate::Ready {
                identity,
                display_name,
            } => (identity, display_name),
            GuestCreate::RateLimited => {
                self.phase = GuestPhase::Idle;
                self.refresh(ctx, true);
                ctx.say("orinks.net is busy. Try again in a minute.");
                return;
            }
            GuestCreate::Failed => {
                self.phase = GuestPhase::Idle;
                self.refresh(ctx, true);
                ctx.say("Could not reach orinks.net. Try again.");
                return;
            }
        };
        let dir = data_dir();
        let record = GuestRecord {
            driver_id: identity.driver_id.clone(),
            display_name: name.clone(),
        };
        if save_identity(&identity).is_err() || online_guest::save_guest(&dir, &record).is_err() {
            self.phase = GuestPhase::Idle;
            self.refresh(ctx, true);
            ctx.audio.play("ui/error");
            ctx.say(
                "This computer could not save the guest driver. Nothing was changed. Check your \
                 password store, then try again.",
            );
            return;
        }
        // No backups for guests; sharing flips only once orinks.net confirms.
        ctx.settings.cloud_saves = false;
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
        // The name is said with the sharing answer, a moment from now, so one
        // line never cuts off the other.
        self.name = Some(name);
        self.phase = GuestPhase::Sharing;
        self.refresh(ctx, true);
        self.share(identity);
    }

    fn share(&mut self, identity: OnlineIdentity) {
        let outcome = self.outcome.clone();
        let transport = online_transport();
        run_worker(self.threaded, "profile-sharing", move || {
            outcome.post(GuestOutcome::Sharing(online_presence::set_profile_sharing(
                &identity,
                true,
                transport.as_ref(),
            )));
        });
    }

    fn finish_sharing(&mut self, ctx: &mut GameContext, answer: &str) {
        self.phase = GuestPhase::Idle;
        let name = self.name.clone().unwrap_or_default();
        if answer == "ok" {
            ctx.settings.online_presence = true;
            ctx.settings.profile_sharing_consent_version = PROFILE_SHARING_CONSENT_VERSION;
            if let Err(e) = ctx.settings.save() {
                log::warn!("Could not save settings: {e}");
            }
            ctx.apply_online_presence();
            ctx.say(&format!("You're {name} on the drivers board."));
        } else {
            ctx.say(&format!(
                "You're {name}, but orinks.net could not turn Profile sharing on, so nothing \
                 is public yet. Profile sharing on the Online menu tries again."
            ));
        }
        ctx.pop_state();
    }
}

impl Menu for GuestSetupState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![
            MenuItem::new(
                Label::dynamic(|s: &Self, _| s.start_label()),
                |s: &mut Self, ctx| s.start(ctx),
            )
            .help("Gets a guest name from orinks.net and turns Profile sharing on."),
            MenuItem::new("Hear what gets shared", |_: &mut Self, ctx| {
                ctx.say(GUEST_DISCLOSURE)
            }),
            MenuItem::new("Cancel", |s: &mut Self, ctx| s.go_back(ctx)),
        ]
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let current = self.current_text(ctx);
        ctx.say(&format!("{}. {GUEST_DISCLOSURE} {current}", Self::TITLE));
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
        let Some(outcome) = self.outcome.take() else {
            return;
        };
        match outcome {
            GuestOutcome::Created(created) => self.finish_created(ctx, created),
            GuestOutcome::Sharing(answer) => self.finish_sharing(ctx, answer),
        }
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        match self.phase {
            GuestPhase::Idle => menu_default_go_back(self, ctx),
            // The request is already out; leaving now would lose the guest
            // it makes, or strand Profile sharing half set.
            _ => ctx.say("Still working. Stay here for the result."),
        }
    }
}

impl_state_for_menu!(GuestSetupState);

// -- GuestNameState ---------------------------------------------------------------------------

/// "Guest name": hear the name, ask for a new one as often as you like.
pub struct GuestNameState {
    pub menu: MenuCore<Self>,
    pub pending: bool,
    pub outcome: Mailbox<GuestRename>,
    pub threaded: bool,
}

impl GuestNameState {
    pub const TITLE: &'static str = "Guest name";

    pub fn new(_ctx: &mut GameContext) -> Self {
        Self {
            menu: MenuCore::new(Self::TITLE),
            pending: false,
            outcome: Mailbox::new(),
            threaded: true,
        }
    }

    pub fn ask(&mut self, ctx: &mut GameContext) {
        if self.pending {
            ctx.say("Still asking.");
            return;
        }
        let Some(identity) = load_identity().filter(|_| current_guest().is_some()) else {
            ctx.say("This computer isn't driving as a guest.");
            return;
        };
        self.pending = true;
        self.refresh(ctx, true);
        let outcome = self.outcome.clone();
        let transport = online_transport();
        run_worker(self.threaded, "guest-rename", move || {
            outcome.post(online_guest::rename_guest(&identity, transport.as_ref()));
        });
    }
}

impl Menu for GuestNameState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![
            MenuItem::new(
                Label::dynamic(|s: &Self, _| {
                    if s.pending {
                        "Asking for a new name".to_string()
                    } else {
                        "Ask for a new name".to_string()
                    }
                }),
                |s: &mut Self, ctx| s.ask(ctx),
            )
            .help("orinks.net picks another name at random. Ask again for another."),
            MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)),
        ]
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let name = current_guest().map(|g| g.display_name).unwrap_or_default();
        let current = self.current_text(ctx);
        ctx.say(&format!("{}. You're {name}. {current}", Self::TITLE));
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
        let Some(answer) = self.outcome.take() else {
            return;
        };
        self.pending = false;
        self.refresh(ctx, true);
        match answer {
            GuestRename::Renamed(name) => {
                if let (Some(dir), Some(mut record)) = (data_dir_if_allowed(), current_guest()) {
                    record.display_name = name.clone();
                    if let Err(e) = online_guest::save_guest(&dir, &record) {
                        log::warn!("Could not save the guest name: {e}");
                    }
                }
                ctx.audio.play("ui/menu_select");
                ctx.say(&format!("You're {name} now."));
            }
            GuestRename::RateLimited => ctx.say("That's a lot of names. Try again in a minute."),
            GuestRename::NotAGuest => ctx.say(
                "orinks.net no longer has this guest. Connect an orinks.net account from the \
                 Online menu.",
            ),
            GuestRename::Failed => ctx.say("Could not reach orinks.net. Try again."),
        }
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        // The new name is saved on this computer only when the answer lands.
        if self.pending {
            ctx.say("Still asking. Stay here for the new name.");
            return;
        }
        menu_default_go_back(self, ctx);
    }
}

impl_state_for_menu!(GuestNameState);
