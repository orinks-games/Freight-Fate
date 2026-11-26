//! The cycle restart at a rest stop: the row after the full night (or the
//! lot's night), a forecast on the first pick and the rest on the second.

use ff_core::sim::hos::{RestartOffer, RestartPlace};

use super::*;
use crate::states::driving_rest_states::rest_preview::restart_status;

impl RestStopState {
    /// The restart row, while one is on offer; `lot` at a stop that offers
    /// the lot instead of a sleeper spot.
    pub(super) fn restart_item(ctx: &GameContext, lot: bool) -> Option<MenuItem<Self>> {
        let place = if lot {
            RestartPlace::Lot
        } else {
            RestartPlace::Sleeper
        };
        let offer = hos_of(ctx).restart_offer(&ctx.settings.hos_mode);
        let label = offer.label(place)?;
        let help = offer.help(place)?;
        Some(
            MenuItem::new(label, move |s: &mut Self, ctx: &mut GameContext| {
                s.take_restart(ctx, lot)
            })
            .help(help),
        )
    }

    /// Rest until the restart is complete, after the forecast.
    fn take_restart(&mut self, ctx: &mut GameContext, lot: bool) {
        // A restart always gains a fresh cycle, so the forecast never opens
        // on "already rested": no fatigue is at or below this floor.
        if self.confirm_sleep(ctx, SleepChoice::Restart { lot }, -1.0) {
            return;
        }
        let offer = hos_of(ctx).restart_offer(&ctx.settings.hos_mode);
        if offer == RestartOffer::None {
            return;
        }
        let place = if lot {
            RestartPlace::Lot
        } else {
            RestartPlace::Sleeper
        };
        let row = offer.label(place).and_then(|label| {
            self.menu
                .items
                .iter()
                .position(|item| item.text(self, ctx) == label)
        });
        let Some(text) = self.driving.clone().with(ctx, |d, ctx| {
            let engine_off = shut_down_engine(d, ctx);
            let minutes = offer.minutes();
            let logged = if lot { None } else { Some("sleeper_berth") };
            advance_rest_clock(d, ctx, minutes, logged, "");
            settle_lift_before_sleep(d, ctx);
            hos_mut_of(ctx).finish_restart(restart_status(lot));
            let rested = {
                let p = profile_mut_of(ctx);
                p.fatigue = if lot {
                    hos::rest_poor_sleep(p.fatigue, minutes)
                } else {
                    hos::rest_sleep(p.fatigue, minutes)
                };
                Alertness::from_fatigue(p.fatigue) == Alertness::Rested
            };
            format!(
                "{engine_off}{} It is {}. {} {}{}",
                offer.woke_line(rested),
                clock_text(d.trip.local_hour()),
                clocks_reset_text(true),
                deadline_text(d, ctx),
                wake_air_instruction(d)
            )
        }) else {
            return;
        };
        self.save_here(ctx, true);
        ctx.audio.play("ui/notify");
        ctx.say(&text);
        ctx.award_restart_badges(false);
        self.refresh(ctx, true);
        // The row is gone, and the row that slid under the focus is one the
        // driver never chose. Focus the night's row above it, which reads
        // its forecast before it sleeps.
        if let Some(row) = row {
            self.menu.index = row.saturating_sub(1);
        }
    }
}

/// The sentence a full night adds when it left the cycle used up, with its
/// leading space; empty in every other case.
pub(super) fn still_used_up(ctx: &GameContext) -> String {
    hos_of(ctx)
        .cycle_still_used_up_text(&ctx.settings.hos_mode)
        .map(|text| format!(" {text}"))
        .unwrap_or_default()
}
