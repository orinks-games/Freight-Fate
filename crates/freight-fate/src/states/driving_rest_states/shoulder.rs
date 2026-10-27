//! Emergency shoulder sleep warning, shared by full lots and no-stop cases.

use crate::app::{GameContext, Say};
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::driving_core::{perform_shoulder_sleep, secure_truck_for_stopped_menu};
use crate::states::driving_menu_states::DriveRef;

const SHOULDER_INTRO_HELP: &str = "Enter selects, Escape cancels.";

pub struct ShoulderSleepConfirmationState {
    menu: MenuCore<Self>,
    driving: DriveRef,
    pub reason: String,
    pub anchor_mi: Option<f64>,
    /// `ctx._app.state is driving`: whether the drive itself opened this, so
    /// cancelling goes back to the road rather than to a covering menu.
    direct_from_driving: bool,
    /// Held at a chain checkpoint: no ticket or debris, and the lines say
    /// so.
    pub at_chain_checkpoint: bool,
}

impl ShoulderSleepConfirmationState {
    pub fn new(
        driving: DriveRef,
        reason: &str,
        anchor_mi: Option<f64>,
        direct_from_driving: bool,
    ) -> Self {
        ShoulderSleepConfirmationState {
            menu: MenuCore::new("Emergency shoulder sleep").with_intro_help(SHOULDER_INTRO_HELP),
            driving,
            reason: reason.to_string(),
            anchor_mi,
            direct_from_driving,
            at_chain_checkpoint: false,
        }
    }

    /// Sleep while held at a chain checkpoint, from the pause menu. The
    /// officer keeps the truck there, so there is no stop to find first.
    pub fn at_chain_checkpoint(driving: DriveRef, anchor_mi: f64) -> Self {
        let mut state = Self::new(
            driving,
            "Held at the chain checkpoint.",
            Some(anchor_mi),
            false,
        );
        state.menu = MenuCore::new("Sleep at the checkpoint").with_intro_help(SHOULDER_INTRO_HELP);
        state.at_chain_checkpoint = true;
        state
    }

    /// The screen as a covering menu opens it: the drive is not the active
    /// state, so cancelling returns to that menu.
    pub fn from_menu(driving: DriveRef, reason: &str, anchor_mi: Option<f64>) -> Self {
        Self::new(driving, reason, anchor_mi, false)
    }

    fn sleep(&mut self, ctx: &mut GameContext) {
        let secured = self
            .driving
            .with(ctx, secure_truck_for_stopped_menu)
            .unwrap_or(false);
        if !secured {
            ctx.say("Come to a complete stop first.");
            return;
        }
        let anchor = match self.anchor_mi {
            Some(anchor) => anchor,
            None => self.driving.read(|d| d.trip.position_mi).unwrap_or(0.0),
        };
        let at_checkpoint = self.at_chain_checkpoint;
        let text = self
            .driving
            .with(ctx, |d, ctx| {
                perform_shoulder_sleep(d, ctx, anchor, at_checkpoint)
            })
            .unwrap_or_default();
        // Unwind every screen back to the drive itself.
        for _ in 0..16 {
            if self.driving.is_active(ctx) || ctx.stack_len() == 0 {
                break;
            }
            ctx.pop_state_with(true, false);
        }
        ctx.say(&text);
    }
}

impl Menu for ShoulderSleepConfirmationState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        if self.at_chain_checkpoint {
            return vec![
                MenuItem::new("Cancel", |s: &mut Self, ctx| s.go_back(ctx))
                    .help("Back without sleeping."),
                MenuItem::new("Sleep at the checkpoint", |s: &mut Self, ctx| s.sleep(ctx)).help(
                    "Poor rest on the checkpoint apron. Resets hours of service; the deadline \
                     keeps running.",
                ),
            ];
        }
        vec![
            MenuItem::new(
                "Cancel and keep looking for a safe stop",
                |s: &mut Self, ctx| s.go_back(ctx),
            )
            .help("Back without resting here."),
            MenuItem::new("Sleep on the shoulder anyway", |s: &mut Self, ctx| {
                s.sleep(ctx)
            })
            .help(
                "Poor rest, possible ticket, possible minor truck damage, and deadline time \
                 lost.",
            ),
        ]
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let title = self.menu.title.clone();
        let reason = self.reason.clone();
        let current = self.current_text(ctx);
        if self.at_chain_checkpoint {
            ctx.say(&format!(
                "{title}. {reason} 10 hours of poor rest, HOS reset if enforced. The deadline \
                 keeps running. {current}"
            ));
            return;
        }
        ctx.say(&format!(
            "{title}. {reason} Emergency shoulder sleep: 10 hours of poor rest, HOS reset if \
             enforced. Ticket and light damage possible; the deadline keeps running. {current}"
        ));
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        if !self.direct_from_driving {
            ctx.audio.play("ui/menu_back");
            ctx.pop_state();
            return;
        }
        ctx.audio.play("ui/menu_back");
        ctx.pop_state();
        let hint = ctx.control_hint("parking_brake");
        ctx.say_with(
            format!(
                "Shoulder sleep canceled. Back on the road. Parking brake set, {hint} releases \
                 it."
            ),
            Say::new(),
        );
    }
}

impl_state_for_menu!(ShoulderSleepConfirmationState);
