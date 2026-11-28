//! The cycle restart at a rest stop: the row after the full night (or the
//! lot's night, or the motel's), a forecast on the first pick and the rest
//! on the second. The rest itself, [`rest_out_restart`], is shared with the
//! full lot, whose motel row takes it in one pick as its night does.

use ff_core::pyfmt::fmt_grouped;
use ff_core::sim::hos::{RestartOffer, RestartPlace};

use super::*;

impl RestStopState {
    /// The restart row at `place` (the sleeper berth, the lot, or a motel
    /// room), while one is on offer.
    pub(super) fn restart_item(ctx: &GameContext, place: RestartPlace) -> Option<MenuItem<Self>> {
        let offer = hos_of(ctx).restart_offer(&ctx.settings.hos_mode);
        let label = offer.label(place)?;
        let help = offer.help(place)?;
        Some(
            MenuItem::new(label, move |s: &mut Self, ctx: &mut GameContext| {
                s.take_restart(ctx, place)
            })
            .help(help),
        )
    }

    /// Rest until the restart is complete, after the forecast.
    fn take_restart(&mut self, ctx: &mut GameContext, place: RestartPlace) {
        let offer = hos_of(ctx).restart_offer(&ctx.settings.hos_mode);
        if offer == RestartOffer::None {
            return;
        }
        if place == RestartPlace::Motel && !can_pay_for_room(ctx, offer) {
            return;
        }
        // A restart always gains a fresh cycle, so the forecast never opens
        // on "already rested": no fatigue is at or below this floor.
        if self.confirm_sleep(ctx, SleepChoice::Restart(place), -1.0) {
            return;
        }
        let row = offer.label(place).and_then(|label| {
            self.menu
                .items
                .iter()
                .position(|item| item.text(self, ctx) == label)
        });
        let Some(text) = self
            .driving
            .clone()
            .with(ctx, |d, ctx| rest_out_restart(d, ctx, offer, place))
        else {
            return;
        };
        self.save_here(ctx, true);
        ctx.audio.play("ui/notify");
        ctx.say(&text);
        // A motel bed is not the bunk, so no Five-by-Two, as the motel's
        // night; the restart badges are earned anywhere.
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

/// True when the driver can pay for the motel room `offer` needs; says the
/// price and the cash on hand when they cannot.
pub(in crate::states::driving_rest_states) fn can_pay_for_room(
    ctx: &mut GameContext,
    offer: RestartOffer,
) -> bool {
    let cost = offer.motel_cost();
    let money = profile_of(ctx).money();
    if money >= cost {
        return true;
    }
    ctx.audio.play("ui/error");
    ctx.say(&format!(
        "A motel room costs {} dollars and you have {}.",
        fmt_grouped(cost, 0),
        fmt_grouped(money, 0)
    ));
    false
}

/// Rest out the restart `offer` at `place` over the drive: the engine off,
/// the clock, the logbook and the fatigue, and the room paid at a motel.
/// Returns the line that says how it went. The caller saves and speaks it.
pub(in crate::states::driving_rest_states) fn rest_out_restart(
    d: &mut DrivingState,
    ctx: &mut GameContext,
    offer: RestartOffer,
    place: RestartPlace,
) -> String {
    let engine_off = shut_down_engine(d, ctx);
    let minutes = offer.minutes();
    let cost = offer.motel_cost();
    if place == RestartPlace::Motel {
        profile_mut_of(ctx).spend(cost);
    }
    let logged = (place == RestartPlace::Sleeper).then_some("sleeper_berth");
    advance_rest_clock(d, ctx, minutes, logged, "");
    settle_lift_before_sleep(d, ctx);
    hos_mut_of(ctx).finish_restart(place.duty_status());
    let rested = {
        let p = profile_mut_of(ctx);
        p.fatigue = if place == RestartPlace::Lot {
            hos::rest_poor_sleep(p.fatigue, minutes)
        } else {
            hos::rest_sleep(p.fatigue, minutes)
        };
        Alertness::from_fatigue(p.fatigue) == Alertness::Rested
    };
    if place == RestartPlace::Motel {
        return format!(
            "{engine_off}Motel, {} dollars. {} It is {}. {} {} dollars. {}{}",
            fmt_grouped(cost, 0),
            offer.woke_line(rested),
            clock_text(d.trip.local_hour()),
            clocks_reset_text(true),
            fmt_grouped(profile_of(ctx).money(), 0),
            deadline_text(d, ctx),
            wake_air_instruction(d)
        );
    }
    format!(
        "{engine_off}{} It is {}. {} {}{}",
        offer.woke_line(rested),
        clock_text(d.trip.local_hour()),
        clocks_reset_text(true),
        deadline_text(d, ctx),
        wake_air_instruction(d)
    )
}

/// The sentence a full night adds when it left the cycle used up, with its
/// leading space; empty in every other case.
pub(super) fn still_used_up(ctx: &GameContext) -> String {
    hos_of(ctx)
        .cycle_still_used_up_text(&ctx.settings.hos_mode)
        .map(|text| format!(" {text}"))
        .unwrap_or_default()
}
