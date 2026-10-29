//! The fuel island, for every state that can be standing on one
//! (Python's `_FuelPumpMixin`).
//!
//! A truck stop's pumps and its parking lot are separate facilities, and the
//! lot filling up does not lock the island: a driver turned away from an
//! overnight space can still pull to the pumps, fuel, and leave. Keeping the
//! purchase here rather than on `RestStopState` alone is what lets
//! `ParkingFullState` offer it -- before this, a full lot swallowed the whole
//! stop, and an overnight run could pass a row of open pumps and still go
//! dry.

use ff_core::models::loyalty::loyalty_earnings_text;
use ff_core::pyfmt::{fmt_f, fmt_grouped};
use ff_core::sim::trip_models::RoadStop;
use ff_core::sim::vehicle::KG_PER_LB;

use crate::app::GameContext;
use crate::states::base::Menu;
use crate::states::driving::DrivingState;
use crate::states::driving_core::{
    advance_rest_clock, hos_mut_of, player_pays_operating_costs, profile_mut_of, profile_of,
    FUEL_STOP_MIN,
};
use crate::states::driving_menu_states::DriveRef;

pub trait FuelPump: Menu {
    fn drive(&self) -> &DriveRef;
    fn stop(&self) -> &RoadStop;

    fn weight_margin_text(label: &str, margin_kg: f64) -> String {
        let pounds = fmt_grouped((margin_kg.abs() / KG_PER_LB).round(), 0);
        let side = if margin_kg >= 0.0 { "under" } else { "over" };
        format!("{label}: {pounds} pounds {side} the gross-weight limit")
    }

    /// The fuel row, given the drive the caller already holds.
    ///
    /// Takes the drive rather than reaching for it, exactly as
    /// `RestStopState::tire_label` does, because the menus that show this
    /// row build their rows INSIDE `DriveRef::call` -- the drive is already
    /// borrowed there, a second borrow can only fail, and the failure used
    /// to fall through to "tank is full" on a tank that was nearly empty.
    fn fuel_label(&self, ctx: &GameContext, d: &DrivingState) -> String {
        let need = d.trip.truck.specs.fuel_tank_gal - d.trip.truck.fuel_gal;
        if need < 1.0 {
            return "Fuel: tank is full".to_string();
        }
        let margin = Self::weight_margin_text(
            "Full tank",
            d.trip
                .truck
                .gross_weight_margin_after_fuel_kg(d.trip.truck.specs.fuel_tank_gal),
        );
        if !player_pays_operating_costs(&profile_of(ctx).business_status) {
            return format!(
                "Refuel {} gallons on the carrier fuel card. {margin}",
                fmt_f(need, 0)
            );
        }
        let cost = ctx.economy.fuel_cost(d.trip.current_region(), need) + 35.0;
        format!(
            "Refuel {} gallons for {} dollars. {margin}",
            fmt_f(need, 0),
            fmt_grouped(cost, 0)
        )
    }

    fn refuel(&mut self, ctx: &mut GameContext) {
        let Some((mut need, region, engine_on)) = self.drive().with(ctx, |d, _| {
            (
                d.trip.truck.specs.fuel_tank_gal - d.trip.truck.fuel_gal,
                d.trip.current_region().to_string(),
                d.trip.truck.engine_on,
            )
        }) else {
            return;
        };
        // Same bar as a car pump: tractor must be off before the nozzle goes
        // in. Reefer/APU is not this flag and is not required off here.
        if engine_on {
            ctx.audio.play("ui/error");
            ctx.say("Shut the engine off before you fuel.");
            return;
        }
        if need < 1.0 {
            ctx.say("The tank is already full.");
            return;
        }
        let stop = self.stop().clone();
        let carrier_card = !player_pays_operating_costs(&profile_of(ctx).business_status);
        let mut cost = 0.0;
        if !carrier_card {
            cost = ctx.economy.fuel_cost(&region, need) + 35.0;
            if profile_of(ctx).money() < cost {
                let partial_gal =
                    ((profile_of(ctx).money() - 35.0) / ctx.economy.fuel_price(&region)).max(0.0);
                if partial_gal < 5.0 {
                    ctx.audio.play("ui/error");
                    ctx.say("You cannot afford fuel here.");
                    return;
                }
                need = partial_gal;
                cost = ctx.economy.fuel_cost(&region, need) + 35.0;
            }
            profile_mut_of(ctx).spend(cost);
        }
        let margin_kg = self.drive().clone().with(ctx, |d, ctx| {
            d.trip.truck.refuel(Some(need));
            // free showers with fuel
            d.stop_visit(&stop).fueled = true;
            advance_rest_clock(d, ctx, FUEL_STOP_MIN, None, "");
            hos_mut_of(ctx).on_duty(FUEL_STOP_MIN);
            d.trip.truck.gross_weight_margin_kg()
        });
        let margin = Self::weight_margin_text("Gross weight", margin_kg.unwrap_or(0.0));
        ctx.audio.play("vehicle/fuel_pump");

        // Award loyalty points for fueling, then save: saved first, the
        // points and shower credit were lost to a quit before the next save.
        let loyalty_text = {
            let result = profile_mut_of(ctx)
                .loyalty
                .add_fueling(need, None, &stop.name, &region);
            loyalty_earnings_text(need, result.points_earned, &result.rewards)
        };
        self.save_here(ctx, true);

        if carrier_card {
            // the carrier fuel card covers road fuel for company drivers
            ctx.say(&format!(
                "Refueled {} gallons on the carrier fuel card. Fueling took {} minutes. \
                 {margin}. {loyalty_text}",
                fmt_f(need, 0),
                fmt_f(FUEL_STOP_MIN, 0)
            ));
        } else {
            let money = profile_of(ctx).money();
            ctx.say(&format!(
                "Refueled {} gallons for {} dollars. You have {} dollars. Fueling took {} \
                 minutes. {margin}. {loyalty_text}",
                fmt_f(need, 0),
                fmt_grouped(cost, 0),
                fmt_grouped(money, 0),
                fmt_f(FUEL_STOP_MIN, 0)
            ));
        }
        ctx.award_achievement("route_refuel");
        self.refresh(ctx, true);
    }

    fn save_here(&mut self, ctx: &mut GameContext, silent: bool) {
        self.drive().clone().with(ctx, |d, ctx| {
            let snapshot = d.snapshot(ctx);
            let p = profile_mut_of(ctx);
            p.store_truck_condition(&d.trip.truck);
            p.active_trip = Some(snapshot);
        });
        ctx.save_profile();
        if !silent {
            ctx.audio.play("ui/notify");
            let name = self.stop().spoken_name();
            ctx.say(&format!("Saved at {name}."));
        }
    }
}
