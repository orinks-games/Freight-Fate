//! The CAT Scale on a truck stop's lot: pay, weigh, hear the ticket.
//!
//! A stop has one when its listed services carry `scale` -- read from the
//! brand's location page or from OpenStreetMap (`tools/cat_scales.py`). The
//! ticket reads the truck's axle groups from `TruckState::axle_loads`.

use ff_core::pyfmt::{fmt_f, fmt_grouped};

use super::RestStopState;
use crate::app::GameContext;
use crate::states::base::{Menu, MenuItem};
use crate::states::driving::DrivingState;
use crate::states::driving_core::{
    advance_rest_clock, clock_text, deadline_text, hos_mut_of, player_pays_operating_costs,
    profile_mut_of, profile_of,
};
use crate::states::driving_rest_states::fuel_pump::FuelPump;

/// CAT Scale's published U.S. prices. READ from catscale.com, Frequently
/// Asked Questions ("a full price weigh ($15.25) and two reweighs (for a
/// charge of $25.75)"), accessed 2026-09-24.
pub const FIRST_WEIGH_DOLLARS: f64 = 15.25;
pub const REWEIGH_DOLLARS: f64 = 5.25;
/// Same page: a reweigh is the same truck and trailer on the same scale
/// within 24 hours of the full-price ticket. READ.
pub const REWEIGH_WINDOW_H: f64 = 24.0;
/// Pull across, press the call button, walk in for the ticket. ASSUMED:
/// CAT publishes no time.
pub const WEIGH_MIN: f64 = 10.0;

/// "15 dollars and 25 cents": cents said as cents, not rounded away.
fn dollars_and_cents(amount: f64) -> String {
    let cents = (amount * 100.0).round() as i64;
    match cents % 100 {
        0 => format!("{} dollars", fmt_grouped((cents / 100) as f64, 0)),
        rest => format!(
            "{} dollars and {rest} cents",
            fmt_grouped((cents / 100) as f64, 0)
        ),
    }
}

impl RestStopState {
    pub(super) fn has_cat_scale(&self) -> bool {
        self.stop.stop_type != "weigh_station" && self.stop.services.iter().any(|s| s == "scale")
    }

    /// Reweigh price while the full-price ticket from this scale is fresh.
    fn weigh_price(&self, d: &DrivingState, ctx: &GameContext) -> (bool, f64) {
        let now = d.absolute_game_hour(ctx, None);
        let reweigh = self
            .visit
            .full_weigh_h
            .is_some_and(|at| now - at < REWEIGH_WINDOW_H);
        let price = if reweigh {
            REWEIGH_DOLLARS
        } else {
            FIRST_WEIGH_DOLLARS
        };
        (reweigh, price)
    }

    pub(super) fn cat_scale_item(&self, ctx: &GameContext, d: &DrivingState) -> MenuItem<Self> {
        let (reweigh, price) = self.weigh_price(d, ctx);
        let verb = if reweigh { "Reweigh" } else { "Weigh" };
        let label = if player_pays_operating_costs(&profile_of(ctx).business_status) {
            format!("{verb} on the CAT Scale: {}", dollars_and_cents(price))
        } else {
            format!("{verb} on the CAT Scale, carrier billed")
        };
        MenuItem::new(label, |s: &mut Self, ctx| s.weigh(ctx)).help(format!(
            "CAT Scale ticket: steer, drive, trailer, and gross, ahead of a state scale. \
             Reweigh within 24 hours is {}. {} minutes on duty.",
            dollars_and_cents(REWEIGH_DOLLARS),
            fmt_f(WEIGH_MIN, 0)
        ))
    }

    fn weigh(&mut self, ctx: &mut GameContext) {
        let Some((reweigh, price)) = self.driving.read(|d| self.weigh_price(d, ctx)) else {
            return;
        };
        let carrier = !player_pays_operating_costs(&profile_of(ctx).business_status);
        if !carrier && profile_of(ctx).money() < price {
            ctx.audio.play("ui/error");
            ctx.say(&format!(
                "A weigh costs {} and you have {} dollars.",
                dollars_and_cents(price),
                fmt_grouped(profile_of(ctx).money(), 0)
            ));
            return;
        }
        if !carrier {
            profile_mut_of(ctx).spend(price);
        }
        let stop = self.stop.clone();
        let Some(text) = self.driving.clone().with(ctx, |d, ctx| {
            if !reweigh {
                let now = d.absolute_game_hour(ctx, None);
                d.stop_visit(&stop).full_weigh_h = Some(now);
            }
            let ticket = d.trip.truck.axle_loads().ticket_text();
            advance_rest_clock(
                d,
                ctx,
                WEIGH_MIN,
                Some("on_duty_not_driving"),
                "CAT Scale weigh",
            );
            hos_mut_of(ctx).on_duty(WEIGH_MIN);
            let billing = if carrier {
                "Billed to the carrier.".to_string()
            } else {
                format!(
                    "{}. You have {} dollars.",
                    dollars_and_cents(price),
                    fmt_grouped(profile_of(ctx).money(), 0)
                )
            };
            format!(
                "CAT Scale ticket. {ticket} {billing} It is {}. {}",
                clock_text(d.trip.local_hour()),
                deadline_text(d, ctx)
            )
        }) else {
            return;
        };
        self.save_here(ctx, true);
        ctx.audio.play("ui/notify");
        ctx.say(&text);
        self.refresh(ctx, true);
    }
}

#[cfg(test)]
mod tests {
    use super::dollars_and_cents;

    #[test]
    fn prices_speak_their_cents() {
        assert_eq!(dollars_and_cents(15.25), "15 dollars and 25 cents");
        assert_eq!(dollars_and_cents(5.25), "5 dollars and 25 cents");
        assert_eq!(dollars_and_cents(30.0), "30 dollars");
    }
}
