//! Where the co-driver's information lives: the Driver screen, the ELD
//! app, and the C readout.

use ff_core::models::co_driver::{self, CO_DRIVER_SUMMON_LEAD_MIN};
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
        let left = co_driver::shift_left_min(hos_of(ctx), &ctx.settings.hos_mode)?;
        Some((left - CO_DRIVER_SUMMON_LEAD_MIN).max(0.0))
    }

    /// How the player's rest stands while the co-driver drives, and when
    /// they hand back: "Rest: 3.5 hours of 10 hours. Dan hands back in about 6.5
    /// hours." The C readout's hours clause, the Driver screen and the
    /// pause menu all read this.
    pub fn co_driver_clock_clause(&self, ctx: &GameContext) -> Option<String> {
        let team = self.team.as_ref()?;
        let name = team.co_driver.name;
        let hours_left = self.co_driver_minutes_left(ctx)?;
        let Some(rest_left) = self.player_rest_left_min(ctx) else {
            return Some(format!(
                "{name} needs you back in {}.",
                hos::duration_text(hours_left / 60.0)
            ));
        };
        if rest_left <= 0.0 {
            return Some(format!(
                "Your 10-hour rest is in. {name} hands back at the next place to park."
            ));
        }
        let rested = hos::duration_text(co_driver::rested_min(&team.off_wheel_hos) / 60.0);
        if rest_left <= hours_left {
            Some(format!(
                "Rest: {rested} of 10 hours. {name} hands back in about {}.",
                hos::duration_text(rest_left / 60.0)
            ))
        } else {
            Some(format!(
                "Rest: {rested} of 10 hours. {name} needs you back in {}, before your rest is in.",
                hos::duration_text(hours_left / 60.0)
            ))
        }
    }

    /// The co-driver's rows on the Driver screen.
    pub fn team_status_lines(&self, ctx: &GameContext) -> Vec<String> {
        let Some(team) = &self.team else {
            return Vec::new();
        };
        let name = team.co_driver.name;
        let mut lines = Vec::new();
        if team.wheel.is_some() {
            lines.push(format!("Co-driver: {name}, driving"));
            lines.extend(self.co_driver_clock_clause(ctx));
        } else {
            lines.push(self.co_driver_bunk_line(ctx));
        }
        lines.push(format!("About {name}: {}", team.co_driver.bio));
        if !team.last_summary.is_empty() {
            lines.push(format!("Last shift: {}", team.last_summary.join(" ")));
        }
        lines
    }

    /// The co-driver in the sleeper, with hours enforced: how far their own
    /// rest has come and the driving it leaves them, so a hand-over short
    /// of their hours is heard before it is made (tester report,
    /// 2026-11-19: only the hand-over row's help said so).
    fn co_driver_bunk_line(&self, ctx: &GameContext) -> String {
        let Some(team) = &self.team else {
            return String::new();
        };
        let name = team.co_driver.name;
        let co = &team.off_wheel_hos;
        let Some(left) = co_driver::shift_left_min(co, &ctx.settings.hos_mode) else {
            return format!("Co-driver: {name}, in the sleeper");
        };
        if co_driver::fully_rested(co) {
            return format!("Co-driver: {name}, in the sleeper, rested.");
        }
        format!(
            "Co-driver: {name}, in the sleeper. Rest: {} of 10 hours. {} of driving left.",
            hos::duration_text(co_driver::rested_min(co) / 60.0),
            hos::duration_text(left / 60.0)
        )
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
