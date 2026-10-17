//! Where the co-driver's information lives: the Driver screen, the ELD
//! app, and the C readout.

use ff_core::models::co_driver::{CO_DRIVER_SHIFT_MIN, CO_DRIVER_SUMMON_LEAD_MIN};
use ff_core::sim::hos::{self, HosClock};

use crate::app::GameContext;
use crate::states::driving::DrivingState;
use crate::states::driving_core::hos_of;

impl DrivingState {
    /// The player's own logbook, wherever the team has it right now.
    pub fn player_hos_now<'a>(&'a self, ctx: &'a GameContext) -> &'a HosClock {
        match &self.team {
            Some(team) => team.player_hos(ctx),
            None => hos_of(ctx),
        }
    }

    /// Minutes until the co-driver at the wheel needs the player back, with
    /// hours of service enforced.
    pub fn co_driver_minutes_left(&self, ctx: &GameContext) -> Option<f64> {
        self.team.as_ref()?.wheel.as_ref()?;
        let mode = &ctx.settings.hos_mode;
        if hos::is_non_enforced(mode) || hos::limits(mode).is_none() {
            return None;
        }
        let clock = hos_of(ctx);
        let shift = CO_DRIVER_SHIFT_MIN - CO_DRIVER_SUMMON_LEAD_MIN - clock.driving_min;
        let legal = clock
            .remaining_min(mode)
            .map_or(shift, |left| left - CO_DRIVER_SUMMON_LEAD_MIN);
        Some(shift.min(legal).max(0.0))
    }

    /// The C readout's hours clause while the co-driver drives: when they
    /// will need the player back.
    pub fn co_driver_clock_clause(&self, ctx: &GameContext) -> Option<String> {
        let team = self.team.as_ref()?;
        let left = self.co_driver_minutes_left(ctx)?;
        Some(format!(
            "{} needs you back in {}.",
            team.co_driver.name,
            hos::duration_text(left / 60.0)
        ))
    }

    /// The co-driver's rows on the Driver screen.
    pub fn team_status_lines(&self, ctx: &GameContext) -> Vec<String> {
        let Some(team) = &self.team else {
            return Vec::new();
        };
        let name = team.co_driver.name;
        let mut lines = Vec::new();
        if team.wheel.is_some() {
            let back = self
                .co_driver_minutes_left(ctx)
                .map(|left| format!(", needs you back in {}", hos::duration_text(left / 60.0)))
                .unwrap_or_default();
            lines.push(format!("Co-driver: {name}, driving{back}"));
        } else {
            lines.push(format!("Co-driver: {name}, in the sleeper"));
        }
        lines.push(format!("About {name}: {}", team.co_driver.bio));
        if !team.last_summary.is_empty() {
            lines.push(format!("Last shift: {}", team.last_summary.join(" ")));
        }
        lines
    }

    /// The co-driver's logbook line in the ELD app.
    pub fn team_eld_line(&self, ctx: &GameContext) -> Option<String> {
        let team = self.team.as_ref()?;
        let summary = team.co_driver_hos(ctx).summary(&ctx.settings.hos_mode);
        Some(format!(
            "ELD, {}: {}",
            team.co_driver.name,
            summary.trim_end_matches('.')
        ))
    }
}
