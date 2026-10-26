//! Snow chains off the truck stop's shelf, in the states that run chain
//! controls. Before this the terminal garage was the only place to buy a
//! set, so a driver whose chains snapped mid-trip could only wait out the
//! law or turn back (player report, 2026-10-17).

use ff_core::pyfmt::{fmt_f, fmt_grouped};
use ff_core::sim::trip_models::CHAIN_CONTROL_STATES;

use super::RestStopState;
use crate::app::GameContext;
use crate::states::base::{Menu, MenuItem};
use crate::states::driving::DrivingState;
use crate::states::driving_core::{
    advance_rest_clock, clock_text, hos_mut_of, player_pays_operating_costs, profile_mut_of,
    profile_of, ROAD_CHAIN_BUY_MIN, ROAD_CHAIN_SET_COST,
};
use crate::states::driving_rest_states::fuel_pump::FuelPump;

impl RestStopState {
    /// A fuel stop in a chain-control state sells a set when the truck has
    /// none that can go on.
    pub(super) fn sells_chains(&self, ctx: &GameContext, d: &DrivingState) -> bool {
        self.stop.stop_type != "weigh_station"
            && self.stop.actions.iter().any(|a| a == "fuel")
            && d.trip
                .state_code_at(self.stop.at_mi)
                .is_some_and(|code| CHAIN_CONTROL_STATES.contains(&code.as_str()))
            && !d.chains_aboard(ctx)
    }

    pub(super) fn chains_item(&self, ctx: &GameContext) -> MenuItem<Self> {
        let label = if player_pays_operating_costs(&profile_of(ctx).business_status) {
            format!(
                "Buy snow chains: {} dollars",
                fmt_grouped(ROAD_CHAIN_SET_COST, 0)
            )
        } else {
            "Buy snow chains, carrier billed".to_string()
        };
        MenuItem::new(label, |s: &mut Self, ctx| s.buy_chains(ctx)).help(format!(
            "A new set with two spare repair links. {} minutes on duty.",
            fmt_f(ROAD_CHAIN_BUY_MIN, 0)
        ))
    }

    fn buy_chains(&mut self, ctx: &mut GameContext) {
        let carrier = !player_pays_operating_costs(&profile_of(ctx).business_status);
        if !carrier && profile_of(ctx).money() < ROAD_CHAIN_SET_COST {
            ctx.audio.play("ui/error");
            ctx.say(&format!(
                "A set of snow chains costs {} dollars and you have {} dollars.",
                fmt_grouped(ROAD_CHAIN_SET_COST, 0),
                fmt_grouped(profile_of(ctx).money(), 0)
            ));
            return;
        }
        let Some(text) = self.driving.clone().with(ctx, |d, ctx| {
            if !carrier {
                profile_mut_of(ctx).spend(ROAD_CHAIN_SET_COST);
            }
            d.stow_new_chain_set(ctx);
            advance_rest_clock(
                d,
                ctx,
                ROAD_CHAIN_BUY_MIN,
                Some("on_duty_not_driving"),
                "chain set",
            );
            hos_mut_of(ctx).on_duty(ROAD_CHAIN_BUY_MIN);
            let billing = if carrier {
                "Billed to the carrier.".to_string()
            } else {
                format!(
                    "{} dollars. You have {} dollars.",
                    fmt_grouped(ROAD_CHAIN_SET_COST, 0),
                    fmt_grouped(profile_of(ctx).money(), 0)
                )
            };
            format!(
                "A new set of snow chains is in the side box. {billing} It is {}.",
                clock_text(d.trip.local_hour())
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
