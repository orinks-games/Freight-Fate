//! The 2.0 badges that fire outside a delivery: a finished restart, and a
//! hire after a carrier let the driver go. The rules are
//! `ff_core::achievements::career_two`; this is where the game calls them.

use ff_core::achievements::career_two::{rehired_after_let_go, restart_badges};

use super::GameContext;

impl GameContext {
    /// After a rest: Sunday Morning Coming Down once the off-duty time has
    /// restarted the cycle, and Take Me Home, Country Roads too when the
    /// rest was at the driver's own carrier terminal.
    pub fn award_restart_badges(&mut self, at_home_terminal: bool) {
        let Some(p) = self.profile.as_ref() else {
            return;
        };
        for badge in restart_badges(p.hos.restart_complete(), at_home_terminal) {
            self.award_achievement(badge);
        }
    }

    /// Get Back, for a driver a carrier let go who is on another carrier's
    /// payroll now.
    pub fn award_rehire_badge(&mut self) {
        let Some(p) = self.profile.as_ref() else {
            return;
        };
        let record = &p.driving_record;
        if rehired_after_let_go(
            record.carrier_terminations,
            &p.carrier_key,
            &record.let_go_by,
        ) {
            self.award_achievement("carrier_rehired");
        }
    }
}
