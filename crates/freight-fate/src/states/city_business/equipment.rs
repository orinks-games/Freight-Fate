//! The garage's equipment shops: fleet upgrades and the truck dealer.

use ff_core::models::business::is_owner_operator;
use ff_core::models::trucks::{truck_traits, TruckModel, Upgrade, TRUCK_CATALOG, UPGRADE_CATALOG};
use ff_core::pyfmt::fmt_grouped;

use super::save_equipment_change;
use crate::app::GameContext;
use crate::impl_state_for_menu;
use crate::states::base::{Label, Menu, MenuCore, MenuItem};
use crate::states::city::{profile, profile_mut, py_capitalize};

// -- Upgrades ---------------------------------------------------------------------------------

pub struct UpgradeShopState {
    menu: MenuCore<Self>,
}

impl Default for UpgradeShopState {
    fn default() -> Self {
        Self::new()
    }
}

impl UpgradeShopState {
    pub fn new() -> Self {
        UpgradeShopState {
            menu: MenuCore::new("Upgrades").with_intro_help(
                "Upgrades apply to every tractor you own. Enter buys the next tier, F1 is \
                 details. Escape returns to the garage.",
            ),
        }
    }

    fn locked(&mut self, ctx: &mut GameContext) {
        ctx.audio.play("ui/error");
        ctx.say(
            "Upgrades unlock with the owner-operator buy-in, from level 18 under Business status.",
        );
    }

    fn label(ctx: &GameContext, upgrade: &Upgrade) -> String {
        let owned = profile(ctx).upgrades.get(upgrade.key).copied().unwrap_or(0);
        let max_tier = upgrade.max_tier();
        if owned >= max_tier {
            let tiers = if max_tier > 1 {
                format!(", tier {owned} of {max_tier}")
            } else {
                String::new()
            };
            return format!("{}: owned{tiers}", upgrade.label);
        }
        let price = upgrade.prices[owned.max(0) as usize];
        if max_tier > 1 {
            let owned_part = if owned > 0 {
                format!(", tier {owned} owned")
            } else {
                String::new()
            };
            return format!(
                "{}, tier {} of {max_tier}: \
                 {} dollars{owned_part}",
                upgrade.label,
                owned + 1,
                fmt_grouped(price, 0)
            );
        }
        format!("{}: {} dollars", upgrade.label, fmt_grouped(price, 0))
    }

    fn buy(&mut self, ctx: &mut GameContext, upgrade: &'static Upgrade) {
        if !is_owner_operator(&profile(ctx).business_status) {
            ctx.audio.play("ui/error");
            ctx.say("Upgrades unlock with the owner-operator buy-in, from level 18 under Business status.");
            return;
        }
        let owned = profile(ctx).upgrades.get(upgrade.key).copied().unwrap_or(0);
        if owned >= upgrade.max_tier() {
            ctx.say(&format!("{} is already fully installed.", upgrade.label));
            return;
        }
        let price = upgrade.prices[owned.max(0) as usize];
        if profile(ctx).money() < price {
            ctx.audio.play("ui/error");
            ctx.say(&format!(
                "Not enough money. {} costs {} dollars \
                 and you have {}.",
                upgrade.label,
                fmt_grouped(price, 0),
                fmt_grouped(profile(ctx).money(), 0)
            ));
            return;
        }
        let (money, all_owned) = {
            let p = profile_mut(ctx);
            p.spend(price);
            p.upgrades.insert(upgrade.key.to_string(), owned + 1);
            let all_owned = UPGRADE_CATALOG
                .iter()
                .all(|item| p.upgrades.get(item.key).copied().unwrap_or(0) >= item.max_tier());
            (p.money(), all_owned)
        };
        save_equipment_change(ctx);
        ctx.audio.play("ui/cash");
        let tier_part = if upgrade.max_tier() > 1 {
            format!(" tier {}", owned + 1)
        } else {
            String::new()
        };
        ctx.say(&format!(
            "{}{tier_part} installed across your fleet for \
             {} dollars. \
             You have {} dollars left.",
            upgrade.label,
            fmt_grouped(price, 0),
            fmt_grouped(money, 0)
        ));
        ctx.award_achievement("first_upgrade");
        if all_owned {
            ctx.award_achievement("all_upgrades");
        }
        self.refresh(ctx, true);
    }
}

impl Menu for UpgradeShopState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let (money, owner) = {
            let p = profile(ctx);
            (p.money(), is_owner_operator(&p.business_status))
        };
        let current = self.current_text(ctx);
        if owner {
            ctx.say(&format!(
                "Fleet upgrades, for every tractor you own. You have {} dollars. {current}",
                fmt_grouped(money, 0)
            ));
        } else {
            ctx.say(&format!(
                "Upgrades. You have {} dollars. {current}",
                fmt_grouped(money, 0)
            ));
        }
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        if !is_owner_operator(&profile(ctx).business_status) {
            return vec![
                MenuItem::new(
                    "Upgrades locked: carrier-assigned tractor",
                    |s: &mut Self, ctx| s.locked(ctx),
                )
                .help("Upgrades unlock with the owner-operator buy-in, from level 18 under Business status."),
                MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)),
            ];
        }
        let mut items: Vec<MenuItem<Self>> = UPGRADE_CATALOG
            .iter()
            .map(|u| {
                MenuItem::new(
                    Label::dynamic(move |_s: &Self, ctx| Self::label(ctx, u)),
                    move |s: &mut Self, ctx| s.buy(ctx, u),
                )
                .help(u.description)
            })
            .collect();
        items.push(MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)));
        items
    }
}

impl_state_for_menu!(UpgradeShopState);

// -- Trucks ----------------------------------------------------------------------------------

/// The truck dealer with no carrier: the buy-in leases the tractor to a
/// carrier, so it waits for one.
pub const TRUCK_SHOP_NO_CARRIER: &str = "Truck ownership starts with the leased-on \
     owner-operator buy-in, and that needs a carrier to lease to. Apply to a carrier first.";

pub struct TruckShopState {
    menu: MenuCore<Self>,
    /// The dealer-name prefix is only true when the player actually
    /// opened this from the terminal's "Truck dealer" row -- reaching it
    /// from the garage's "Trucks" row would name a location the player
    /// never went to, which is a contradicted navigation cue for a blind
    /// or low-vision player.
    pub at_dealer: bool,
}

impl TruckShopState {
    pub fn new(at_dealer: bool) -> Self {
        TruckShopState {
            menu: MenuCore::new("Trucks").with_intro_help(
                "Owner-operators buy tractors or switch among those they own. Upgrades follow \
                 whichever tractor you drive.",
            ),
            at_dealer,
        }
    }

    fn locked(&mut self, ctx: &mut GameContext) {
        ctx.audio.play("ui/error");
        if profile(ctx).is_unassigned_company_driver() {
            ctx.say(TRUCK_SHOP_NO_CARRIER);
            return;
        }
        ctx.say("Company drivers use the carrier's trucks. Buying trucks unlocks with the owner-operator buy-in, from level 18 under Business status.");
    }

    fn label(ctx: &GameContext, model: &TruckModel) -> String {
        let p = profile(ctx);
        let name = py_capitalize(model.label);
        let traits = truck_traits(model);
        if model.key == p.truck {
            return format!("{name}: currently driving, {traits}");
        }
        if p.visible_owned_trucks().iter().any(|k| k == model.key) {
            return format!("{name}: owned, {traits}, switch to it");
        }
        format!(
            "{name}: {traits}, buy for {} dollars",
            fmt_grouped(model.price, 0)
        )
    }

    fn pick(&mut self, ctx: &mut GameContext, model: &'static TruckModel) {
        if model.key == profile(ctx).truck {
            ctx.say(&format!("You are already driving the {}.", model.label));
            return;
        }
        if !is_owner_operator(&profile(ctx).business_status) {
            ctx.audio.play("ui/error");
            ctx.say("Company drivers use the carrier's trucks. Buying trucks unlocks with the owner-operator buy-in, from level 18 under Business status.");
            return;
        }
        if !profile(ctx).owned_trucks.iter().any(|k| k == model.key) {
            if profile(ctx).money() < model.price {
                ctx.audio.play("ui/error");
                ctx.say(&format!(
                    "Not enough money. The {} costs \
                     {} dollars and you have {}.",
                    model.label,
                    fmt_grouped(model.price, 0),
                    fmt_grouped(profile(ctx).money(), 0)
                ));
                return;
            }
            let owned_count = {
                let p = profile_mut(ctx);
                p.spend(model.price);
                p.owned_trucks.push(model.key.to_string());
                // A truck off the dealer lot is its own rig: fresh wear, full tank.
                p.provision_truck_condition(model.key, Some(model.specs.fuel_tank_gal));
                p.owned_trucks.len()
            };
            ctx.audio.play("ui/cash");
            self.switch_to(ctx, model);
            let money = profile(ctx).money();
            ctx.say(&format!(
                "You bought the {} for {} dollars, now your tractor. You have {} dollars left.",
                model.label,
                fmt_grouped(model.price, 0),
                fmt_grouped(money, 0)
            ));
            if model.key == "heavy_hauler" {
                ctx.award_achievement("heavy_hauler");
            }
            if owned_count >= 3 {
                ctx.award_achievement("three_trucks");
            }
            return;
        }
        ctx.audio.play("vehicle/truck_door");
        self.switch_to(ctx, model);
        ctx.say(&format!("You are now driving the {}.", model.label));
    }

    fn switch_to(&mut self, ctx: &mut GameContext, model: &TruckModel) {
        {
            let p = profile_mut(ctx);
            p.truck = model.key.to_string();
            let fuel = p.truck_fuel_gal().min(p.truck_specs().fuel_tank_gal);
            p.set_truck_fuel_gal(fuel);
        }
        save_equipment_change(ctx);
        self.refresh(ctx, true);
    }
}

impl Menu for TruckShopState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let mut dealer = String::new();
        if self.at_dealer {
            if let Ok(service) = ctx
                .world
                .city_service(&profile(ctx).current_city, "truck_dealer")
            {
                if !service.fallback {
                    dealer = format!("Inside {}. ", service.name);
                }
            }
        }
        let money = profile(ctx).money();
        let current = self.current_text(ctx);
        ctx.say(&format!(
            "{dealer}Trucks. You have {} dollars. {current}",
            fmt_grouped(money, 0)
        ));
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        if profile(ctx).is_unassigned_company_driver() {
            // No carrier, so no assigned tractor, and the buy-in has no
            // carrier to lease to (`owner_operator_eligibility`).
            return vec![
                MenuItem::new("Truck ownership locked: no carrier", |s: &mut Self, ctx| {
                    s.locked(ctx)
                })
                .help(TRUCK_SHOP_NO_CARRIER),
                MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)),
            ];
        }
        if !is_owner_operator(&profile(ctx).business_status) {
            return vec![
                MenuItem::new(
                    "Truck ownership locked: carrier-assigned tractor",
                    |s: &mut Self, ctx| s.locked(ctx),
                )
                .help(
                    "Unlocks with the owner-operator buy-in, from level 18 under Business status.",
                ),
                MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)),
            ];
        }
        // A tractor the catalog prices at nothing (the standard rig) is only
        // listed to a driver who owns one; anyone else would buy it free.
        let owned = profile(ctx).owned_trucks.clone();
        let mut items: Vec<MenuItem<Self>> = TRUCK_CATALOG
            .values()
            .filter(|m| m.price > 0.0 || owned.iter().any(|k| k == m.key))
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

impl_state_for_menu!(TruckShopState);
