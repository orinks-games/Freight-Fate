//! The owner-operator buy-in: the driver picks a sleeper tractor and buys
//! it at its catalog price, keeping the working capital in the bank.
//!
//! Each row asks twice: the first press says the price and what is left,
//! the second buys. Moving to another row disarms it.

use ff_core::models::business::{owner_operator_eligibility, OWNER_OPERATOR_WORKING_CAPITAL};
use ff_core::models::business_stage::BusinessEvent;
use ff_core::models::buy_in::{buy_in_tractors, can_afford_buy_in};
use ff_core::models::trailers::DEFAULT_TRAILER_PROGRAMS;
use ff_core::models::trucks::{truck_traits, TruckModel};
use ff_core::pyfmt::fmt_grouped;

use super::save_business_change;
use crate::app::{GameContext, Say};
use crate::impl_state_for_menu;
use crate::states::base::{Label, Menu, MenuCore, MenuItem};
use crate::states::city::{profile, profile_mut, py_capitalize};

pub struct BuyInState {
    menu: MenuCore<Self>,
    /// The tractor whose row was pressed once; the next press on it buys.
    armed: Option<&'static str>,
}

impl Default for BuyInState {
    fn default() -> Self {
        Self::new()
    }
}

impl BuyInState {
    pub fn new() -> Self {
        BuyInState {
            menu: MenuCore::new("Owner-operator buy-in"),
            armed: None,
        }
    }

    fn capital() -> String {
        fmt_grouped(OWNER_OPERATOR_WORKING_CAPITAL, 0)
    }

    fn label(ctx: &GameContext, model: &TruckModel) -> String {
        let name = py_capitalize(model.label);
        let traits = truck_traits(model, ctx.settings.imperial());
        let price = fmt_grouped(model.price, 0);
        if profile(ctx).active_truck_key() == model.key {
            format!("{name}, the tractor you drive now: {traits}, {price} dollars")
        } else {
            format!("{name}: {traits}, {price} dollars")
        }
    }

    fn pick(&mut self, ctx: &mut GameContext, model: &'static TruckModel) {
        let (ok, reasons) = owner_operator_eligibility(profile(ctx));
        if !ok {
            self.armed = None;
            ctx.audio.play("ui/error");
            ctx.say(&format!(
                "Owner-operator path locked. {}",
                reasons.join(" ")
            ));
            return;
        }
        let money = profile(ctx).money();
        if !can_afford_buy_in(money, model) {
            self.armed = None;
            ctx.audio.play("ui/error");
            ctx.say(&format!(
                "Not enough money. The {} costs {} dollars, and {} dollars stays in the bank as \
                 working capital. You have {}.",
                model.label,
                fmt_grouped(model.price, 0),
                Self::capital(),
                fmt_grouped(money, 0)
            ));
            return;
        }
        if self.armed != Some(model.key) {
            self.armed = Some(model.key);
            let again = if ctx.touch_hints() {
                "Double tap again to buy it."
            } else {
                "Press Enter again to buy it."
            };
            ctx.say(&format!(
                "{}, {} dollars. You would have {} dollars left. {again}",
                py_capitalize(model.label),
                fmt_grouped(model.price, 0),
                fmt_grouped(money - model.price, 0)
            ));
            return;
        }
        self.armed = None;
        let money = buy(profile_mut(ctx), model);
        save_business_change(ctx);
        ctx.audio.play("ui/cash");
        // Back to Business status, which now reads as an owner-operator; the
        // result is said after its entry line so it is the last thing heard.
        ctx.pop_state();
        ctx.say(&format!(
            "Leased-on owner-operator status unlocked. You bought the {} for {} dollars and \
             have {} dollars left.",
            model.label,
            fmt_grouped(model.price, 0),
            fmt_grouped(money, 0)
        ));
        ctx.award_achievement("owner_operator_buyin");
    }
}

/// The buy-in itself: pay the price, own the tractor, lease on. The tractor
/// dispatch assigned keeps its condition record; any other is fresh off the
/// lot, full tank, and the assigned one goes back to the yard. Returns the
/// cash left.
fn buy(p: &mut ff_core::models::profile::Profile, model: &'static TruckModel) -> f64 {
    let assigned = p.active_truck_key();
    p.spend(model.price);
    p.apply_business_event(BusinessEvent::BuyIn);
    if model.key != assigned {
        p.provision_truck_condition(model.key, Some(model.specs.fuel_tank_gal));
    }
    if !p.owned_trucks.iter().any(|k| k == model.key) {
        p.owned_trucks.push(model.key.to_string());
    }
    p.buy_in_paid = model.price;
    if p.trailer_programs.is_empty() {
        p.trailer_programs = DEFAULT_TRAILER_PROGRAMS
            .iter()
            .map(|s| s.to_string())
            .collect();
    }
    p.truck = model.key.to_string();
    p.dispatch_board_cache = None;
    p.money()
}

impl Menu for BuyInState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let money = profile(ctx).money();
        let current = self.current_text(ctx);
        ctx.say(&format!(
            "Owner-operator buy-in. You have {} dollars. {} dollars stays in the bank as \
             working capital. {current}",
            fmt_grouped(money, 0),
            Self::capital()
        ));
    }

    /// Any move to a row reads it; reading a row disarms the last press.
    fn speak_current(&mut self, ctx: &mut GameContext) {
        self.armed = None;
        let text = self.current_text(ctx);
        ctx.say_with(text, Say::new().review(false));
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = buy_in_tractors()
            .into_iter()
            .map(|m| {
                MenuItem::new(
                    Label::dynamic(move |_s: &Self, ctx| Self::label(ctx, m)),
                    move |s: &mut Self, ctx| s.pick(ctx, m),
                )
                .help(m.description)
            })
            .collect();
        items.push(MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)));
        items
    }
}

impl_state_for_menu!(BuyInState);
