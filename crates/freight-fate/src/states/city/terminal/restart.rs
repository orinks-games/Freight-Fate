//! The terminal's cycle restart row: one pick rests the whole restart, or
//! what is left of it, in the bunk room.

use ff_core::sim::hos::{RestartOffer, RestartPlace};

use super::*;

/// Parked at the carrier's home terminal, where a restart earns its badge.
pub(super) fn at_home_terminal(ctx: &GameContext) -> bool {
    let p = profile(ctx);
    !p.carrier_key.is_empty()
        && ctx.world.resolve_city_key(&p.current_city)
            == ctx.world.resolve_city_key(&p.home_terminal_city)
}

impl CityMenuState {
    /// The restart row, while one is on offer (`HosClock::restart_offer`).
    pub(super) fn restart_item(ctx: &GameContext) -> Option<MenuItem<Self>> {
        let offer = profile(ctx).hos.restart_offer(&ctx.settings.hos_mode);
        let label = offer.label(RestartPlace::Terminal)?;
        let help = offer.help(RestartPlace::Terminal)?;
        Some(MenuItem::new(label, |s: &mut Self, ctx| s.take_restart(ctx)).help(help))
    }

    /// Rest until the restart is complete: no confirmation, since there is
    /// no load and no deadline, and the label says what happens.
    pub fn take_restart(&mut self, ctx: &mut GameContext) {
        let offer = profile(ctx).hos.restart_offer(&ctx.settings.hos_mode);
        if offer == RestartOffer::None {
            return;
        }
        self.confirm_sleep_rested = false;
        let row = self.restart_row_index(ctx, offer);
        let (before_fatigue, start, end) = {
            let p = profile_mut(ctx);
            let before_fatigue = p.fatigue;
            let start = p.game_hours;
            p.game_hours += offer.minutes() / 60.0;
            (before_fatigue, start, p.game_hours)
        };
        record_city_duty(ctx, "sleeper_berth", start, end, "cycle restart");
        {
            let p = profile_mut(ctx);
            let minutes = p.hos.finish_restart("sleeper_berth");
            p.fatigue = hos::rest_sleep(p.fatigue, minutes);
            let day = p.market_day();
            p.market.advance_to(day);
        }
        ctx.save_profile();
        ctx.audio.play("ui/notify");
        let zone = local_zone(ctx);
        let hour = to_local(profile(ctx).game_hours, zone).rem_euclid(24.0);
        ctx.say(&offer.rested_line(&clock_text(hour), time_of_day(hour)));
        if Alertness::from_fatigue(before_fatigue) < Alertness::Severe {
            ctx.award_achievement("sleep_before_exhaustion");
        }
        ctx.award_restart_badges(at_home_terminal(ctx));
        self.refresh(ctx, true);
        // The row is gone, and the row that slid under the focus is one the
        // driver never chose. Focus the sleep row above it, which asks
        // before it sleeps a rested driver.
        if let Some(row) = row {
            self.menu.index = row.saturating_sub(1);
        }
    }

    /// Where the restart row sits in the menu as it stands.
    fn restart_row_index(&self, ctx: &GameContext, offer: RestartOffer) -> Option<usize> {
        let label = offer.label(RestartPlace::Terminal)?;
        self.menu
            .items
            .iter()
            .position(|item| item.text(self, ctx) == label)
    }
}
