//! Terminal fueling, including short-wallet fills.

use super::*;

impl GarageState {
    pub fn refuel(&mut self, ctx: &mut GameContext) {
        let tank = Self::tank_gal(ctx);
        let need = tank - profile(ctx).truck_fuel_gal();
        if need < 1.0 {
            ctx.say("The tank is already full.");
            return;
        }
        let minutes = town_hand_work_min(ctx, TERMINAL_FUEL_MIN);
        if !player_pays_operating_costs(&profile(ctx).business_status) {
            let (start, end) = {
                let p = profile_mut(ctx);
                p.set_truck_fuel_gal(tank);
                let start = p.game_hours;
                p.game_hours += minutes / 60.0;
                p.hos.on_duty(minutes);
                (start, p.game_hours)
            };
            crate::states::city::city_on_duty_fatigue(ctx, start, end);
            ctx.save_profile();
            ctx.audio.play("vehicle/fuel_pump");
            ctx.say(&format!("Tank filled, {} minutes.", fmt_f(minutes, 0)));
            ctx.award_achievement("route_refuel");
            self.refresh(ctx, true);
            return;
        }
        let cost = ctx.economy.fuel_cost(&Self::region(ctx), need);
        if profile(ctx).money() < cost {
            self.partial_refuel(ctx, tank);
            return;
        }
        let (start, end, money) = {
            let p = profile_mut(ctx);
            p.spend(cost);
            p.set_truck_fuel_gal(tank);
            let start = p.game_hours;
            p.game_hours += minutes / 60.0;
            (start, p.game_hours, p.money())
        };
        record_terminal_duty(ctx, start, end, "terminal fuel");
        profile_mut(ctx).hos.on_duty(minutes);
        ctx.save_profile();
        ctx.audio.play("vehicle/fuel_pump");
        ctx.say(&format!(
            "Tank filled. {} dollars. You have {} dollars left.",
            fmt_grouped(cost, 0),
            fmt_grouped(money, 0)
        ));
        ctx.award_achievement("route_refuel");
        self.refresh(ctx, true);
    }

    fn partial_refuel(&mut self, ctx: &mut GameContext, tank: f64) {
        let region = Self::region(ctx);
        let price = ctx.economy.fuel_price(&region);
        let gallons = if price > 0.0 {
            profile(ctx).money() / price
        } else {
            0.0
        };
        if gallons < 1.0 {
            ctx.audio.play("ui/error");
            ctx.say("Not enough money for one gallon of fuel.");
            return;
        }
        let cost = ctx.economy.fuel_cost(&region, gallons);
        let minutes = town_hand_work_min(ctx, TERMINAL_FUEL_MIN);
        let (start, end, money) = {
            let p = profile_mut(ctx);
            p.spend(cost);
            let fuel = tank.min(p.truck_fuel_gal() + gallons);
            p.set_truck_fuel_gal(fuel);
            let start = p.game_hours;
            p.game_hours += minutes / 60.0;
            (start, p.game_hours, p.money())
        };
        record_terminal_duty(ctx, start, end, "terminal fuel");
        profile_mut(ctx).hos.on_duty(minutes);
        ctx.save_profile();
        ctx.audio.play("vehicle/fuel_pump");
        ctx.say(&format!(
            "Partial fuel: added {} gallons for \
             {} dollars. \
             You have {} dollars left.",
            fmt_f(gallons, 0),
            fmt_grouped(cost, 0),
            fmt_grouped(money, 0)
        ));
        ctx.award_achievement("route_refuel");
        self.refresh(ctx, true);
    }
}
