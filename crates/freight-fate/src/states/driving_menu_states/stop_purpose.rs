//! The list T opens while rolling: what the planned stop is for.
//!
//! Each row names a purpose and the distance to the stop it would plan, so
//! the driver can weigh "fuel in 12 miles" against "fuel and sleep in 60"
//! before choosing. Enter closes the list and plans that stop over the road
//! the driver returns to.

use crate::app::{GameContext, Say};
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::driving_events::stop_plan::StopPlanPurpose;
use crate::states::driving_menu_states::DriveRef;

const STOP_PURPOSE_INTRO_HELP: &str =
    "Up and Down choose what the stop is for, Enter plans it, Escape goes back to driving.";

pub struct StopPurposeState {
    menu: MenuCore<Self>,
    driving: DriveRef,
    /// Each purpose and the miles to the stop it would plan, `None` when no
    /// stop ahead serves it. Read from the drive when the list opened; the
    /// drive waits under the list, so they hold until a row is chosen.
    rows: Vec<(StopPlanPurpose, Option<f64>)>,
    focus: StopPlanPurpose,
}

impl StopPurposeState {
    pub fn new(
        ctx: &GameContext,
        rows: Vec<(StopPlanPurpose, Option<f64>)>,
        focus: StopPlanPurpose,
    ) -> Self {
        Self::with_drive(DriveRef::active(ctx), rows, focus)
    }

    /// The same list over a drive the caller already shares (tests).
    pub fn with_drive(
        driving: DriveRef,
        rows: Vec<(StopPlanPurpose, Option<f64>)>,
        focus: StopPlanPurpose,
    ) -> Self {
        StopPurposeState {
            menu: MenuCore::new("Plan a stop").with_intro_help(STOP_PURPOSE_INTRO_HELP),
            driving,
            rows,
            focus,
        }
    }

    fn plan(&mut self, ctx: &mut GameContext, purpose: StopPlanPurpose) {
        ctx.pop_state();
        self.driving.with(ctx, |drive, ctx| {
            let previous = ctx.player_asked_begin();
            drive.plan_stop_for(ctx, purpose);
            ctx.player_asked_end(previous);
        });
    }
}

impl Menu for StopPurposeState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        if let Some(index) = self.rows.iter().position(|(p, _)| *p == self.focus) {
            self.menu.index = index;
        }
        self.rows
            .iter()
            .map(|&(purpose, ahead)| {
                // Whole miles, but tenths close in, so a stop half a mile on
                // never reads as 0 miles.
                let distance = ahead.map(|miles| ctx.settings.distance_text(miles, miles < 10.0));
                MenuItem::new(
                    purpose.row_label(distance.as_deref()),
                    move |s: &mut Self, ctx: &mut GameContext| s.plan(ctx, purpose),
                )
                .help(purpose.help())
            })
            .collect()
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        ctx.audio.play("ui/menu_back");
        ctx.pop_state();
        ctx.say_with("Back to driving.", Say::queued());
    }
}

impl_state_for_menu!(StopPurposeState);
