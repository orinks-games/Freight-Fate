//! The co-driver's share at the settlement.

use ff_core::models::business_constants::is_owner_operator;
use ff_core::models::co_driver;
use ff_core::pyfmt::{fmt_grouped, round_py_n};

use crate::app::GameContext;
use crate::states::driving::DrivingState;
use crate::states::driving_core::profile_mut_of;
use crate::states::driving_menu_states::ArrivalState;

impl ArrivalState {
    /// Take the co-driver's share of their miles off `net_pay`, say so in
    /// one line, and count the team load. Returns the experience rate for
    /// the load: 1 when the player drove it all.
    pub(crate) fn settle_team_share(
        &mut self,
        ctx: &mut GameContext,
        d: &DrivingState,
        business_status: &str,
        net_pay: &mut f64,
    ) -> f64 {
        let Some(team) = d.team.as_ref().filter(|team| team.co_driver_mi >= 0.5) else {
            return 1.0;
        };
        let total_mi = d.job.distance_mi.max(team.co_driver_mi);
        let split = co_driver::team_split(business_status, *net_pay, total_mi, team.co_driver_mi);
        *net_pay = round_py_n(*net_pay - split.co_driver_pay, 2);
        let name = team.co_driver.name;
        let miles = ctx.settings.distance_text(team.co_driver_mi, false);
        let share = if is_owner_operator(business_status) {
            format!(
                "their {} percent",
                (co_driver::OWNER_CO_DRIVER_PAY_SHARE * 100.0).round()
            )
        } else {
            "their half of the pay".to_string()
        };
        let line = format!(
            "{name} drove {miles} of this load: {share} is {} dollars, and those miles earn you \
             half the experience.",
            fmt_grouped(split.co_driver_pay, 0)
        );
        self.summary_parts.push(line.clone());
        self.team_line = Some(line);
        self.team_miles = team.co_driver_mi;
        self.team_note = Some(format!(
            "Team load: {name} drove {} of the miles",
            ff_core::pyfmt::fmt_f(team.co_driver_mi, 0)
        ));
        let key = team.co_driver.key;
        let p = profile_mut_of(ctx);
        let status = p.business_status.clone();
        co_driver::record_team_load(&mut p.achievement_stats, key, &status);
        split.xp_rate
    }
}
