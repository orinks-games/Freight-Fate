//! Walking the cattle trailer at a stop: the only way to get an animal that
//! is down back on its feet.

use super::RestStopState;
use crate::app::{GameContext, Say};
use crate::states::base::{Menu, MenuItem};
use crate::states::driving_core::{accrue_on_duty_fatigue, advance_rest_clock, hos_mut_of};

/// Climb the side, count the cattle on their feet, prod one up. ASSUMED:
/// no rule or trade source gives a time; the trade guides say to check
/// within the first hour and at every stop.
pub const CHECK_CATTLE_MIN: f64 = 5.0;

impl RestStopState {
    pub(super) fn check_cattle_item(&self) -> MenuItem<Self> {
        MenuItem::new("Check the cattle", |s: &mut Self, ctx| s.check_cattle(ctx))
            .help("Walk the trailer and count the cattle on their feet. 5 minutes on duty.")
    }

    fn check_cattle(&mut self, ctx: &mut GameContext) {
        let Some(text) = self.driving.clone().with(ctx, |d, ctx| {
            let exempt = d.livestock().is_some_and(|load| load.hours_exempt());
            accrue_on_duty_fatigue(d, ctx, CHECK_CATTLE_MIN);
            advance_rest_clock(
                d,
                ctx,
                CHECK_CATTLE_MIN,
                Some("on_duty_not_driving"),
                "checked the cattle",
            );
            if exempt {
                hos_mut_of(ctx).exempt(CHECK_CATTLE_MIN);
            } else {
                hos_mut_of(ctx).on_duty(CHECK_CATTLE_MIN);
            }
            let finding = d.check_the_cattle()?;
            Some(format!("{finding} {}", d.livestock_limit_left_text()))
        }) else {
            return;
        };
        let Some(text) = text else {
            return;
        };
        ctx.audio.play("ui/notify");
        ctx.say(&text);
        ctx.say_with(self.current_text(ctx), Say::queued().review(false));
    }
}
