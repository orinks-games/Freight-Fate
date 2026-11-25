//! The short stays a stop offers whatever its lot holds: the 30-minute
//! break and the food and coffee break. Shared by `RestStopState` and
//! `ParkingFullState`, because a full lot turns away the overnight stay
//! only ([`ff_core::sim::stop_lot::StopLot`]).

use ff_core::achievements::increment_stat;
use ff_core::sim::hos;
use ff_core::sim::stimulant;

use crate::app::GameContext;
use crate::states::base::MenuItem;
use crate::states::driving_core::{
    advance_rest_clock, clock_text, deadline_text, hos_mut_of, profile_mut_of,
};
use crate::states::driving_rest_states::fuel_pump::FuelPump;

pub const BREAK_ROW: &str = "Take a 30-minute break";
pub const FOOD_ROW: &str = "Food and coffee break";
pub const SAVE_ROW: &str = "Save at this stop";

pub trait ShortStay: FuelPump + Sized + 'static {
    fn break_item() -> MenuItem<Self> {
        MenuItem::new(BREAK_ROW, |s: &mut Self, ctx| s.take_break(ctx)).help(
            "Satisfies the 30-minute break rule and eases fatigue. Clock and deadline \
             advance half an hour.",
        )
    }

    fn food_item() -> MenuItem<Self> {
        MenuItem::new(FOOD_ROW, |s: &mut Self, ctx| s.food_break(ctx)).help(
            "Fifteen minutes off duty. Coffee eases fatigue a little, but the lift \
             wears off in two hours, and a second coffee before then adds nothing. Does \
             not satisfy the 30-minute break rule.",
        )
    }

    fn save_item() -> MenuItem<Self> {
        MenuItem::new(SAVE_ROW, |s: &mut Self, ctx| s.save_here(ctx, false))
            .help("Saves the drive at this stop.")
    }

    fn take_break(&mut self, ctx: &mut GameContext) {
        let Some(text) = self.drive().clone().with(ctx, |d, ctx| {
            advance_rest_clock(d, ctx, 30.0, Some("off_duty"), "");
            hos_mut_of(ctx).take_break(30.0);
            {
                let p = profile_mut_of(ctx);
                p.fatigue = hos::rest_break(p.fatigue);
            }
            format!(
                "You took a 30-minute break. It is {}. Break requirement reset. {}",
                clock_text(d.trip.local_hour()),
                deadline_text(d, ctx)
            )
        }) else {
            return;
        };
        // Twenty-five proper breaks. The 30-minute one is the break that
        // counts -- the 15-minute food and coffee stop eases fatigue but
        // resets nothing, so it is not one of these. Counted before the
        // save, so a quit afterwards cannot lose it.
        let breaks = increment_stat(profile_mut_of(ctx), "breaks_taken");
        self.save_here(ctx, true);
        ctx.audio.play("ui/notify");
        ctx.say(&text);
        ctx.award_achievement("break_taken");
        if breaks >= 25 {
            ctx.award_achievement("coffee_regular");
        }
        // The break counts toward a 10-hour reset; the reset row says so.
        self.refresh(ctx, true);
    }

    fn food_break(&mut self, ctx: &mut GameContext) {
        let Some(text) = self.drive().clone().with(ctx, |d, ctx| {
            advance_rest_clock(d, ctx, 15.0, Some("off_duty"), "food and coffee");
            hos_mut_of(ctx).take_break(15.0);
            // The lift is a loan, paid back when the coffee wears off; a
            // second cup before then lifts nothing.
            let now_h = d.absolute_game_hour(ctx, None);
            let lifted = profile_mut_of(ctx).take_stimulant(
                hos::FATIGUE_COFFEE_RELIEF,
                now_h,
                stimulant::COFFEE_BREAK_LOAN_H,
                stimulant::COFFEE_WORN_OFF,
            );
            if lifted > 0.0 {
                format!(
                    "Coffee break. {}. Eases fatigue. Does not count as your 30-minute break. {}",
                    clock_text(d.trip.local_hour()),
                    deadline_text(d, ctx)
                )
            } else {
                format!(
                    "Coffee break. {}. No lift while the last coffee lasts. Does not count as \
                     your 30-minute break. {}",
                    clock_text(d.trip.local_hour()),
                    deadline_text(d, ctx)
                )
            }
        }) else {
            return;
        };
        self.save_here(ctx, true);
        ctx.audio.play("ui/notify");
        ctx.say(&text);
        self.refresh(ctx, true);
    }
}
