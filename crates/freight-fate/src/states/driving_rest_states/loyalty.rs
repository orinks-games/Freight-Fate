//! Loyalty program reward redemption menu (`LoyaltyRewardsState`).

use ff_core::data::buffs::buffs_for_stop;
use ff_core::models::loyalty::reward_cost_text;
use ff_core::sim::trip_models::RoadStop;

use crate::app::GameContext;
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::driving_core::{profile_mut_of, profile_of};
use crate::states::driving_menu_states::DriveRef;

const LOYALTY_INTRO_HELP: &str = "Enter selects, Escape goes back.";

pub struct LoyaltyRewardsState {
    menu: MenuCore<Self>,
    driving: DriveRef,
    pub stop: RoadStop,
}

impl LoyaltyRewardsState {
    pub fn new(driving: DriveRef, stop: RoadStop) -> Self {
        LoyaltyRewardsState {
            menu: MenuCore::new("Loyalty rewards").with_intro_help(LOYALTY_INTRO_HELP),
            driving,
            stop,
        }
    }

    /// Whether this stop sells the shower a reward pays for, and whether it
    /// is already free this visit (fuel, or a reward already redeemed).
    fn shower_here(&self) -> (bool, bool) {
        let actions: Vec<&str> = self.stop.actions.iter().map(String::as_str).collect();
        let sold = buffs_for_stop(&self.stop.name, &actions)
            .iter()
            .any(|buff| buff.free_with_fuel);
        let free = self
            .driving
            .read(|d| {
                let visit = d.stop_visit(&self.stop);
                visit.fueled || visit.free_shower
            })
            .unwrap_or(false);
        (sold, free)
    }

    /// The reward reaches the shower row: free on this visit, and saved.
    /// Spending the credit or points used to be all it did -- the shower
    /// still charged full price (2026-09-28).
    fn grant_free_shower(&mut self, ctx: &mut GameContext) {
        let stop = self.stop.clone();
        self.driving.with(ctx, |d, ctx| {
            d.stop_visit(&stop).free_shower = true;
            let snapshot = d.snapshot(ctx);
            profile_mut_of(ctx).active_trip = Some(snapshot);
        });
        ctx.save_profile();
    }

    fn use_shower_credit(&mut self, ctx: &mut GameContext) {
        if profile_mut_of(ctx).loyalty.use_shower_credit() {
            self.grant_free_shower(ctx);
            ctx.audio.play("ui/notify");
            ctx.say("Shower credit used. The shower is free.");
            self.refresh(ctx, true);
        } else {
            ctx.audio.play("ui/error");
            ctx.say("No shower credits available.");
        }
    }

    fn redeem_shower(&mut self, ctx: &mut GameContext) {
        let result = profile_mut_of(ctx).loyalty.redeem_reward("shower");
        if result.success {
            self.grant_free_shower(ctx);
            ctx.audio.play("ui/notify");
            ctx.say(&format!(
                "Shower redeemed. The shower is free. {} points spent, {} points remaining.",
                result.points_spent.unwrap_or(0),
                ff_core::pyfmt::fmt_f(result.points_remaining, 0)
            ));
            self.refresh(ctx, true);
        } else {
            ctx.audio.play("ui/error");
            ctx.say("Not enough points for a shower.");
        }
    }
}

impl Menu for LoyaltyRewardsState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let loyalty = &profile_of(ctx).loyalty;
        let shower_credits = loyalty.shower_credits;
        let can_shower = loyalty.can_redeem("shower");
        let (shower_sold, shower_free) = self.shower_here();
        // A reward is offered only where it buys something: a shower this
        // stop sells and is not already giving away. The parking, food and
        // laundry discounts spent points on nothing -- the game charges for
        // none of those -- so they wait until something does.
        let offer_shower = shower_sold && !shower_free;
        let mut items: Vec<MenuItem<Self>> = Vec::new();

        // Shower credits option
        if offer_shower && shower_credits > 0 {
            items.push(
                MenuItem::new(
                    format!("Use shower credit, {shower_credits} available"),
                    |s: &mut Self, ctx| s.use_shower_credit(ctx),
                )
                .help("A shower credit from fueling 50 gallons or more."),
            );
        }

        // Point redemption options
        if offer_shower && can_shower {
            items.push(
                MenuItem::new(
                    format!("Redeem {}", reward_cost_text("shower")),
                    |s: &mut Self, ctx| s.redeem_shower(ctx),
                )
                .help("Loyalty points for a free shower."),
            );
        }

        // Only true when the shower is on offer and the balance falls short.
        // With no shower to buy, the entry line's balance is all there is.
        if items.is_empty() && offer_shower {
            items.push(
                MenuItem::inert("No rewards available, more points needed")
                    .help("Fuel at truck stops to earn loyalty points."),
            );
        }

        items.push(
            MenuItem::new("Back to truck stop", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Return to the truck stop menu."),
        );
        items
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let summary = profile_of(ctx).loyalty.summary();
        let title = self.menu.title.clone();
        let name = self.stop.spoken_name();
        ctx.say(&format!("{title}. {summary} At {name}."));
    }
}

impl_state_for_menu!(LoyaltyRewardsState);
