//! Handing the wheel over and taking it back, from the pause menu.

use ff_core::models::business_constants::is_owner_operator;
use ff_core::models::co_driver::{self, Pairing};

use crate::app::{GameContext, Say};
use crate::states::base::MenuItem;
use crate::states::driving::DrivingState;
use crate::states::driving_core::profile_of;
use crate::states::driving_pause_states::PauseMenuState;

use super::TeamMode;

impl DrivingState {
    /// Who would take the wheel: this load's co-driver, or the one dispatch
    /// would pair.
    pub fn team_pairing(&self, ctx: &GameContext) -> Pairing {
        if let Some(team) = &self.team {
            return Pairing {
                co_driver: team.co_driver,
                reason: team.reason,
            };
        }
        let profile = profile_of(ctx);
        co_driver::pair(
            &profile.achievement_stats,
            &profile.business_status,
            profile.career.level(),
            self.trip_seed,
        )
    }

    /// Whether the pause menu offers the handover now.
    pub fn handover_offered(&self, ctx: &GameContext) -> bool {
        // A co-driver back in the bunk before their own rest is done has no
        // hours to give.
        let rested = self
            .team
            .as_ref()
            .is_none_or(|team| !co_driver::shift_over(&team.off_wheel_hos, &ctx.settings.hos_mode));
        rested
            && !self.co_driver_driving()
            && self.pull_over.is_none()
            && self.hazard_deadline.is_none()
            && self.team_available(ctx)
    }
}

/// The terms, as the handover rows' help reads them.
fn terms_text(ctx: &GameContext, name: &str) -> String {
    let owner = is_owner_operator(&profile_of(ctx).business_status);
    let pay = if owner {
        format!(
            "You pay {name} {} percent of the settlement for the miles they drive.",
            (co_driver::OWNER_CO_DRIVER_PAY_SHARE * 100.0).round()
        )
    } else {
        format!("Miles {name} drives pay you half.")
    };
    format!(
        "{pay} They earn you half the experience, and count as sleeper-berth time for your hours."
    )
}

impl PauseMenuState {
    /// The co-driver rows, placed after Resume driving.
    pub(crate) fn team_rows(
        &self,
        ctx: &GameContext,
        d: &DrivingState,
    ) -> Vec<MenuItem<PauseMenuState>> {
        if d.co_driver_driving() {
            return vec![MenuItem::new("Take the wheel back", |s: &mut Self, ctx| {
                s.take_the_wheel_back(ctx)
            })
            .help("Wakes you and gives you the wheel.")];
        }
        if !d.handover_offered(ctx) {
            return Vec::new();
        }
        let pairing = d.team_pairing(ctx);
        let name = pairing.co_driver.name;
        let carrier = ff_core::models::career::carrier_name_of(profile_of(ctx));
        let who = co_driver::pairing_line(&pairing, &carrier);
        let terms = terms_text(ctx, name);
        vec![
            MenuItem::new(
                format!("Hand the wheel to {name} and ride along"),
                |s: &mut Self, ctx| s.hand_over(ctx, TeamMode::RideAlong),
            )
            .help(format!(
                "{who} You rest in the sleeper and hear the road. {terms}"
            )),
            MenuItem::new(
                format!("Hand the wheel to {name} and sleep"),
                |s: &mut Self, ctx| s.hand_over(ctx, TeamMode::SleepThrough),
            )
            .help(format!(
                "{who} You wake when {name} needs you, with a summary of the drive. {terms}"
            )),
        ]
    }

    fn hand_over(&mut self, ctx: &mut GameContext, mode: TeamMode) {
        let line = self
            .drive()
            .with(ctx, |d, ctx| {
                d.hand_wheel_to_co_driver(ctx, mode);
                d.team_pairing_line(ctx)
            })
            .unwrap_or_default();
        ctx.audio.play("ui/unpause");
        ctx.resume_event_speech();
        ctx.pop_state();
        ctx.player_asked(|ctx| ctx.say_with(line, Say::queued().review(false)));
    }

    fn take_the_wheel_back(&mut self, ctx: &mut GameContext) {
        self.drive().with(ctx, |d, ctx| d.take_wheel_back(ctx));
        ctx.audio.play("ui/unpause");
        ctx.resume_event_speech();
        ctx.pop_state();
        ctx.say_with("You have the wheel.", Say::queued().review(false));
    }
}
