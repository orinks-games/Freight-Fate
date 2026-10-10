//! Business-status, truck, upgrade, and trailer program menus (port of
//! `freight_fate/states/city_business.py`).

use ff_core::models::business::{
    authority_activation_eligibility, authority_readiness_eligibility, business_path_label,
    business_status_summary, has_authority_readiness, has_weigh_station_transponder,
    is_owner_operator, next_business_unlock, owner_operator_eligibility, status_label,
    weigh_station_transponder_eligibility, AUTHORITY_ACTIVATION_COST, AUTHORITY_READY_RESERVE,
    INDEPENDENT_AUTHORITY, LEASED_OWNER_OPERATOR, OWNER_OPERATOR_BUY_IN,
    WEIGH_STATION_TRANSPONDER_SIGNUP_FEE,
};
use ff_core::models::career::PendingCredential;
use ff_core::models::credentials::{
    course_eligibility, course_help_text, course_offer_text, credential, Credential, CREDENTIALS,
};
use ff_core::models::enforcement::HOURS_PER_DAY;
use ff_core::models::solvency::{apply_return_to_company_driving, company_return_buy_back};
use ff_core::models::trailers::{TrailerType, DEFAULT_TRAILER_PROGRAMS, TRAILER_CATALOG};
use ff_core::models::trucks::{TruckModel, Upgrade, TRUCK_CATALOG, UPGRADE_CATALOG};
use ff_core::pyfmt::{fmt_f, fmt_grouped};
use ff_core::sim::driving_modes::tuning_for_time_scale;
use ff_core::sim::hos;
use ff_core::sim::timezones::to_local;

use crate::app::GameContext;
use crate::impl_state_for_menu;
use crate::meaningful_play::MeaningfulPlayReason;
use crate::states::base::{Label, Menu, MenuCore, MenuItem};
use crate::states::city::{city_local_zone, profile, profile_mut, py_capitalize};

mod carrier_move;

fn save_business_change(ctx: &mut GameContext) {
    ctx.mark_meaningful_play(MeaningfulPlayReason::BusinessChanged);
    ctx.save_profile();
}

fn save_equipment_change(ctx: &mut GameContext) {
    ctx.mark_meaningful_play(MeaningfulPlayReason::EquipmentChanged);
    ctx.save_profile();
}

// -- Business status -----------------------------------------------------------------------

pub struct BusinessStatusState {
    menu: MenuCore<Self>,
    /// "Go back to company driving" was pressed once; the next press on it
    /// does it. Hands back every tractor, so it is never a single Enter.
    return_armed: bool,
    /// The carrier a "Move to" row was pressed for once; the next press on
    /// the same row moves. There is no moving back to the training fleet.
    move_armed: Option<&'static str>,
}

impl Default for BusinessStatusState {
    fn default() -> Self {
        Self::new()
    }
}

impl BusinessStatusState {
    pub fn new() -> Self {
        BusinessStatusState {
            menu: MenuCore::new("Business status").with_intro_help(
                "Enter repeats a line, or buys in when qualified. Escape returns to the \
                 terminal.",
            ),
            return_armed: false,
            move_armed: None,
        }
    }

    fn summary_label(ctx: &GameContext) -> String {
        format!(
            "Current status: {}",
            status_label(&profile(ctx).business_status)
        )
    }

    fn summary(&mut self, ctx: &mut GameContext) {
        self.return_armed = false;
        let text = business_status_summary(profile(ctx));
        ctx.say(&text);
    }

    fn rank_status(&mut self, ctx: &mut GameContext) {
        self.return_armed = false;
        let text = business_path_label(profile(ctx));
        ctx.say(&text);
    }

    fn next_unlock(&mut self, ctx: &mut GameContext) {
        self.return_armed = false;
        let text = next_business_unlock(profile(ctx));
        ctx.say(&text);
    }

    /// The driver says no to the buy-in and means it: the row stays, the
    /// three reminders that repeat the offer -- next unlock, the summary,
    /// the career plan -- stop, and the plan reads on down the company
    /// ladder. Reversible from the same screen.
    fn stay_company_driver(&mut self, ctx: &mut GameContext) {
        {
            let p = profile_mut(ctx);
            p.owner_operator_declined = true;
            p.dispatch_board_cache = None;
        }
        save_business_change(ctx);
        let carrier = ff_core::models::career::carrier_name_of(profile(ctx));
        ctx.say(&format!(
            "Staying a company driver with {carrier}. The career plan stops pointing you at \
             the buy-in. It stays open here under Business status if you change your mind."
        ));
        self.refresh(ctx, true);
    }

    fn reopen_owner_operator_plan(&mut self, ctx: &mut GameContext) {
        {
            let p = profile_mut(ctx);
            p.owner_operator_declined = false;
            p.dispatch_board_cache = None;
        }
        save_business_change(ctx);
        ctx.say(&format!(
            "Owner-operator plan reopened. {}",
            next_business_unlock(profile(ctx))
        ));
        self.refresh(ctx, true);
    }

    /// Hands every tractor and trailer back and takes a company seat again.
    /// Two presses: the first says what goes and for how much, the second
    /// does it.
    fn return_to_company_driving(&mut self, ctx: &mut GameContext) {
        if !self.return_armed {
            self.return_armed = true;
            let carrier = ff_core::models::career::carrier_name_of(profile(ctx));
            let buy_back = company_return_buy_back(profile(ctx));
            ctx.say(&format!(
                "Going back to company driving hands every tractor and trailer you own back \
                 to {carrier} for {} dollars, and puts you in a carrier tractor on company \
                 wages. Press Enter again to do it.",
                fmt_grouped(buy_back, 0)
            ));
            self.refresh(ctx, true);
            return;
        }
        self.return_armed = false;
        let lines = apply_return_to_company_driving(profile_mut(ctx));
        // Same choice as Stay a company driver: titles and goals follow the
        // company ladder; the buy-in stays open under Business status.
        profile_mut(ctx).owner_operator_declined = true;
        save_business_change(ctx);
        ctx.audio.play("ui/cash");
        ctx.say(&lines.join(" "));
        self.refresh(ctx, true);
    }

    /// What the transponder subscription is still waiting on.
    ///
    /// Its own reader rather than `next_unlock`: that speaks the next
    /// BUSINESS unlock, which is a different question from why this one
    /// item is greyed, and answering the wrong question is how a locked
    /// row teaches a player nothing.
    fn locked_transponder(&mut self, ctx: &mut GameContext, reasons: &[String]) {
        let joined = reasons.join(" ");
        let body = if joined.is_empty() {
            "Not available yet.".to_string()
        } else {
            joined
        };
        ctx.say(&format!("Weigh station transponder. {body}"));
    }

    fn become_owner_operator(&mut self, ctx: &mut GameContext) {
        self.return_armed = false;
        let (ok, reasons) = owner_operator_eligibility(profile(ctx));
        if !ok {
            ctx.audio.play("ui/error");
            ctx.say(&format!(
                "Owner-operator path locked. {}",
                reasons.join(" ")
            ));
            self.refresh(ctx, true);
            return;
        }
        let money = {
            let p = profile_mut(ctx);
            p.spend(OWNER_OPERATOR_BUY_IN);
            let assigned = p.active_truck_key();
            p.business_status = LEASED_OWNER_OPERATOR.to_string();
            p.owner_operator_declined = false;
            if !p.owned_trucks.contains(&assigned) {
                p.owned_trucks.push(assigned.clone());
            }
            if p.trailer_programs.is_empty() {
                p.trailer_programs = DEFAULT_TRAILER_PROGRAMS
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
            }
            p.truck = assigned;
            p.dispatch_board_cache = None;
            p.money()
        };
        save_business_change(ctx);
        ctx.audio.play("ui/cash");
        ctx.say(&format!(
            "Leased-on owner-operator status unlocked. Paid {} dollars toward your first \
             tractor, {} dollars working capital left. Loads pay higher gross, and your \
             business pays fuel, repairs, maintenance reserve, insurance, trailer program, \
             truck payment reserve, and settlement fees.",
            fmt_grouped(OWNER_OPERATOR_BUY_IN, 0),
            fmt_grouped(money, 0)
        ));
        ctx.award_achievement("owner_operator_buyin");
        self.refresh(ctx, true);
    }

    fn set_authority_readiness(&mut self, ctx: &mut GameContext) {
        let (ok, reasons) = authority_readiness_eligibility(profile(ctx));
        if !ok {
            ctx.audio.play("ui/error");
            ctx.say(&format!("Authority prep locked. {}", reasons.join(" ")));
            self.refresh(ctx, true);
            return;
        }
        let money = {
            let p = profile_mut(ctx);
            p.spend(AUTHORITY_READY_RESERVE);
            p.authority_readiness = true;
            p.dispatch_board_cache = None;
            p.money()
        };
        save_business_change(ctx);
        ctx.audio.play("ui/cash");
        ctx.say(&format!(
            "Authority prep reserve set aside: {} dollars. You have {} dollars left. Own \
             authority unlocks after the delivery, reputation, trailer program, and cash \
             gates.",
            fmt_grouped(AUTHORITY_READY_RESERVE, 0),
            fmt_grouped(money, 0)
        ));
        self.refresh(ctx, true);
    }

    fn subscribe_transponder(&mut self, ctx: &mut GameContext) {
        let (ok, reasons) = weigh_station_transponder_eligibility(profile(ctx));
        if !ok {
            ctx.audio.play("ui/error");
            ctx.say(&format!(
                "Transponder subscription locked. {}",
                reasons.join(" ")
            ));
            self.refresh(ctx, true);
            return;
        }
        let money = {
            let p = profile_mut(ctx);
            p.spend(WEIGH_STATION_TRANSPONDER_SIGNUP_FEE);
            p.weigh_station_transponder = true;
            p.money()
        };
        save_business_change(ctx);
        ctx.audio.play("ui/cash");
        ctx.say(&format!(
            "Weigh station transponder active. Paid {} dollars, {} dollars left. A clean \
             truck gets a weigh-in-motion check at most open scales instead of pulling in, \
             for a small per-mile settlement reserve.",
            fmt_grouped(WEIGH_STATION_TRANSPONDER_SIGNUP_FEE, 0),
            fmt_grouped(money, 0)
        ));
        self.refresh(ctx, true);
    }

    fn activate_authority(&mut self, ctx: &mut GameContext) {
        let (ok, reasons) = authority_activation_eligibility(profile(ctx));
        if !ok {
            ctx.audio.play("ui/error");
            ctx.say(&format!("Own authority locked. {}", reasons.join(" ")));
            self.refresh(ctx, true);
            return;
        }
        let money = {
            let p = profile_mut(ctx);
            p.spend(AUTHORITY_ACTIVATION_COST);
            p.business_status = INDEPENDENT_AUTHORITY.to_string();
            p.dispatch_board_cache = None;
            p.money()
        };
        save_business_change(ctx);
        ctx.audio.play("ui/cash");
        ctx.say(&format!(
            "Own authority active. Startup cost \
             {} dollars. You have \
             {} dollars left. Dispatch now lists direct freight. \
             Settlement includes insurance, compliance, trailer, truck, and \
             factoring costs.",
            fmt_grouped(AUTHORITY_ACTIVATION_COST, 0),
            fmt_grouped(money, 0)
        ));
        ctx.award_achievement("authority_active");
        self.refresh(ctx, true);
    }
}

impl Menu for BusinessStatusState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let summary = business_status_summary(profile(ctx));
        let current = self.current_text(ctx);
        ctx.say(&format!("Business status. {summary} {current}"));
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let p = profile(ctx);
        let mut items = vec![
            MenuItem::new(
                Label::dynamic(|_s: &Self, ctx| Self::summary_label(ctx)),
                |s: &mut Self, ctx| s.summary(ctx),
            )
            .help("Current business status and tradeoffs."),
            MenuItem::new("Carrier and rank", |s: &mut Self, ctx| s.rank_status(ctx))
                .help("Carrier, rank, and career stage."),
            MenuItem::new("Next business unlock", |s: &mut Self, ctx| {
                s.next_unlock(ctx)
            })
            .help("The next career or business unlock."),
        ];
        if !is_owner_operator(&p.business_status) {
            let (ok, _reasons) = owner_operator_eligibility(p);
            if ok {
                items.push(
                    MenuItem::new(
                        format!(
                            "Buy into leased-on owner-operator: {} dollars",
                            fmt_grouped(OWNER_OPERATOR_BUY_IN, 0)
                        ),
                        |s: &mut Self, ctx| s.become_owner_operator(ctx),
                    )
                    .help("Higher revenue, but your business pays operating costs."),
                );
                if p.owner_operator_declined {
                    items.push(
                        MenuItem::new("Reopen the owner-operator plan", |s: &mut Self, ctx| {
                            s.reopen_owner_operator_plan(ctx)
                        })
                        .help("The career plan points at the buy-in again."),
                    );
                } else {
                    items.push(
                        MenuItem::new("Stay a company driver", |s: &mut Self, ctx| {
                            s.stay_company_driver(ctx)
                        })
                        .help(
                            "Keep the carrier's tractor and wages. The reminders about the \
                             buy-in stop; the buy-in itself stays open here.",
                        ),
                    );
                }
            } else {
                items.push(
                    MenuItem::new("Owner-operator path locked", |s: &mut Self, ctx| {
                        s.summary(ctx)
                    })
                    .help("The remaining requirements."),
                );
            }
            items.extend(self.carrier_move_items(ctx));
        } else {
            items.push(
                MenuItem::new(
                    Label::dynamic(|s: &Self, _ctx| {
                        if s.return_armed {
                            "Go back to company driving: press Enter again to confirm".to_string()
                        } else {
                            "Go back to company driving".to_string()
                        }
                    }),
                    |s: &mut Self, ctx| s.return_to_company_driving(ctx),
                )
                .help(
                    "The carrier takes your tractors and trailers back and pays you for them. \
                     Company wages again, in a carrier tractor. Asks twice.",
                ),
            );
            if p.business_status == INDEPENDENT_AUTHORITY {
                items.push(
                    MenuItem::new("Own authority active", |s: &mut Self, ctx| s.summary(ctx)).help(
                        "Direct freight is available. Settlement includes \
                             insurance, compliance, and factoring costs.",
                    ),
                );
            } else if has_authority_readiness(p) {
                items.push(
                    MenuItem::new("Authority prep reserve: set", |s: &mut Self, ctx| {
                        s.summary(ctx)
                    })
                    .help("The prep reserve for own-authority startup is set aside."),
                );
                let (ok, _reasons) = authority_activation_eligibility(p);
                if ok {
                    items.push(
                        MenuItem::new(
                            format!(
                                "Activate own authority: {} dollars",
                                fmt_grouped(AUTHORITY_ACTIVATION_COST, 0)
                            ),
                            |s: &mut Self, ctx| s.activate_authority(ctx),
                        )
                        .help("Direct freight, higher gross revenue, more business overhead."),
                    );
                } else {
                    items.push(
                        MenuItem::new("Own authority locked", |s: &mut Self, ctx| {
                            s.next_unlock(ctx)
                        })
                        .help("The remaining own-authority requirements."),
                    );
                }
            } else {
                let (ok, _reasons) = authority_readiness_eligibility(p);
                if ok {
                    items.push(
                        MenuItem::new(
                            format!(
                                "Commit {} dollars to authority prep",
                                fmt_grouped(AUTHORITY_READY_RESERVE, 0)
                            ),
                            |s: &mut Self, ctx| s.set_authority_readiness(ctx),
                        )
                        .help("Money set aside for the own-authority activation gate."),
                    );
                } else {
                    items.push(
                        MenuItem::new("Authority prep locked", |s: &mut Self, ctx| {
                            s.next_unlock(ctx)
                        })
                        .help("The remaining authority prep requirements."),
                    );
                }
            }
            if has_weigh_station_transponder(p) {
                items.push(
                    MenuItem::new(
                        "Weigh station transponder: subscribed",
                        |s: &mut Self, ctx| s.summary(ctx),
                    )
                    .help(
                        "Open scales run a weigh-in-motion check on this \
                         truck instead of demanding every truck pull in.",
                    ),
                );
            } else {
                let (ok, reasons) = weigh_station_transponder_eligibility(p);
                if ok {
                    items.push(
                        MenuItem::new(
                            format!(
                                "Subscribe to weigh station transponder: \
                                 {} dollars",
                                fmt_grouped(WEIGH_STATION_TRANSPONDER_SIGNUP_FEE, 0)
                            ),
                            |s: &mut Self, ctx| s.subscribe_transponder(ctx),
                        )
                        .help(
                            "A clean truck can be waved past most open scales \
                             instead of pulling in. Adds a small per-mile \
                             settlement reserve once active.",
                        ),
                    );
                } else {
                    // Locked, but SHOWN -- the same shape as the authority
                    // items above. Without this the subscription appeared
                    // only once the driver could already afford it, so the
                    // owner-operator with most to gain from knowing the
                    // transponder exists was the one told nothing about it,
                    // and the eligibility reasons were computed here and
                    // thrown away (owner, 2026-08-21).
                    let locked = reasons.clone();
                    items.push(
                        MenuItem::new(
                            "Weigh station transponder locked",
                            move |s: &mut Self, ctx: &mut GameContext| {
                                s.locked_transponder(ctx, &locked)
                            },
                        )
                        .help("What the transponder still needs."),
                    );
                }
            }
        }
        items.push(MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)));
        items
    }
}

impl_state_for_menu!(BusinessStatusState);

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
                "Upgrades apply to every tractor you own. Enter buys the next tier, F1 says \
                 what it does. Escape returns to the garage.",
            ),
        }
    }

    fn locked(&mut self, ctx: &mut GameContext) {
        ctx.audio.play("ui/error");
        ctx.say("Upgrades unlock after the leased-on owner-operator buy-in.");
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
            ctx.say("Upgrades unlock after the leased-on owner-operator buy-in.");
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
                .help("Upgrades unlock after the leased-on owner-operator buy-in."),
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
        ctx.say("Truck ownership unlocks after the leased-on owner-operator buy-in.");
    }

    fn label(ctx: &GameContext, model: &TruckModel) -> String {
        let p = profile(ctx);
        let name = py_capitalize(model.label);
        let specs = &model.specs;
        let traits = format!(
            "{} thousand newton meters torque, \
             {} gallon tank",
            fmt_f(specs.max_torque_nm / 1000.0, 1),
            fmt_f(specs.fuel_tank_gal, 0)
        );
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
            ctx.say("Truck purchases unlock after the leased-on owner-operator buy-in.");
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
        if !is_owner_operator(&profile(ctx).business_status) {
            return vec![
                MenuItem::new(
                    "Truck ownership locked: carrier-assigned tractor",
                    |s: &mut Self, ctx| s.locked(ctx),
                )
                .help("Unlocks after the leased-on owner-operator buy-in."),
                MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)),
            ];
        }
        let mut items: Vec<MenuItem<Self>> = TRUCK_CATALOG
            .values()
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

// -- Trailers --------------------------------------------------------------------------------

pub struct TrailerProgramState {
    menu: MenuCore<Self>,
}

impl Default for TrailerProgramState {
    fn default() -> Self {
        Self::new()
    }
}

impl TrailerProgramState {
    pub fn new() -> Self {
        TrailerProgramState {
            menu: MenuCore::new("Trailers").with_intro_help(
                "Owner-operators start with the dry van program and add specialty programs. \
                 Own authority buys trailers outright. Escape returns to the garage.",
            ),
        }
    }

    fn locked(&mut self, ctx: &mut GameContext) {
        ctx.audio.play("ui/error");
        ctx.say(
            "Trailer programs unlock after the leased-on owner-operator buy-in. The carrier \
             provides trailers.",
        );
    }

    fn label(ctx: &GameContext, trailer: &TrailerType) -> String {
        let p = profile(ctx);
        let owned = p.visible_owned_trailers();
        if p.business_status == INDEPENDENT_AUTHORITY {
            if owned.iter().any(|k| k == trailer.key) {
                return format!("{}: owned trailer", trailer.label);
            }
            return format!(
                "{}: buy trailer for {} dollars",
                trailer.label,
                fmt_grouped(trailer.purchase_price, 0)
            );
        }
        let programs = p.active_trailer_programs();
        if programs.iter().any(|k| k == trailer.key) {
            if DEFAULT_TRAILER_PROGRAMS.contains(&trailer.key) {
                return format!("{}: included carrier trailer program", trailer.label);
            }
            return format!("{}: leased program active", trailer.label);
        }
        format!(
            "{}: lease program for {} dollars",
            trailer.label,
            fmt_grouped(trailer.lease_deposit, 0)
        )
    }

    fn select(&mut self, ctx: &mut GameContext, trailer: &'static TrailerType) {
        if profile(ctx).business_status == INDEPENDENT_AUTHORITY {
            self.buy_trailer(ctx, trailer);
            return;
        }
        self.lease(ctx, trailer);
    }

    fn lease(&mut self, ctx: &mut GameContext, trailer: &TrailerType) {
        if !is_owner_operator(&profile(ctx).business_status) {
            ctx.audio.play("ui/error");
            ctx.say("Trailer programs unlock after the leased-on owner-operator buy-in.");
            return;
        }
        if profile(ctx)
            .active_trailer_programs()
            .iter()
            .any(|k| k == trailer.key)
        {
            ctx.say(&format!(
                "{} trailer program is already active.",
                trailer.label
            ));
            return;
        }
        if profile(ctx).money() < trailer.lease_deposit {
            ctx.audio.play("ui/error");
            ctx.say(&format!(
                "Not enough money. {} trailer program costs \
                 {} dollars and you have \
                 {}.",
                trailer.label,
                fmt_grouped(trailer.lease_deposit, 0),
                fmt_grouped(profile(ctx).money(), 0)
            ));
            return;
        }
        let money = {
            let p = profile_mut(ctx);
            p.spend(trailer.lease_deposit);
            let mut programs = p.active_trailer_programs();
            programs.push(trailer.key.to_string());
            p.trailer_programs = programs;
            p.dispatch_board_cache = None;
            p.money()
        };
        save_equipment_change(ctx);
        ctx.audio.play("ui/cash");
        ctx.say(&format!(
            "{} trailer program active for {} dollars. You have {} dollars left. Matching \
             cargo now appears on the dispatch board.",
            trailer.label,
            fmt_grouped(trailer.lease_deposit, 0),
            fmt_grouped(money, 0)
        ));
        self.refresh(ctx, true);
    }

    fn buy_trailer(&mut self, ctx: &mut GameContext, trailer: &TrailerType) {
        if profile(ctx).business_status != INDEPENDENT_AUTHORITY {
            ctx.audio.play("ui/error");
            ctx.say("Trailer purchases unlock after own authority.");
            return;
        }
        if profile(ctx)
            .visible_owned_trailers()
            .iter()
            .any(|k| k == trailer.key)
        {
            ctx.say(&format!("You already own a {} trailer.", trailer.label));
            return;
        }
        if profile(ctx).money() < trailer.purchase_price {
            ctx.audio.play("ui/error");
            ctx.say(&format!(
                "Not enough money. The {} trailer costs \
                 {} dollars and you have \
                 {}.",
                trailer.label,
                fmt_grouped(trailer.purchase_price, 0),
                fmt_grouped(profile(ctx).money(), 0)
            ));
            return;
        }
        let money = {
            let p = profile_mut(ctx);
            p.spend(trailer.purchase_price);
            let mut owned = p.visible_owned_trailers();
            owned.push(trailer.key.to_string());
            p.owned_trailers = owned;
            p.dispatch_board_cache = None;
            p.money()
        };
        save_equipment_change(ctx);
        ctx.audio.play("ui/cash");
        ctx.say(&format!(
            "{} trailer purchased for \
             {} dollars. You have \
             {} dollars left. Matching direct freight now uses \
             an owned-trailer reserve at settlement.",
            trailer.label,
            fmt_grouped(trailer.purchase_price, 0),
            fmt_grouped(money, 0)
        ));
        self.refresh(ctx, true);
    }
}

impl Menu for TrailerProgramState {
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
            "Trailers. You have {} dollars. {current}",
            fmt_grouped(money, 0)
        ));
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        if !is_owner_operator(&profile(ctx).business_status) {
            return vec![
                MenuItem::new(
                    "Trailer programs locked: carrier-provided trailers",
                    |s: &mut Self, ctx| s.locked(ctx),
                )
                .help("The carrier supplies the trailer for every load."),
                MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)),
            ];
        }
        let mut items: Vec<MenuItem<Self>> = TRAILER_CATALOG
            .iter()
            .map(|t| {
                MenuItem::new(
                    Label::dynamic(move |_s: &Self, ctx| Self::label(ctx, t)),
                    move |s: &mut Self, ctx| s.select(ctx, t),
                )
                .help(t.description)
            })
            .collect();
        items.push(MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)));
        items
    }
}

impl_state_for_menu!(TrailerProgramState);

// -- Licenses and training (the credential ladder) ---------------------------------------------

pub struct EndorsementCourseState {
    menu: MenuCore<Self>,
}

impl Default for EndorsementCourseState {
    fn default() -> Self {
        Self::new()
    }
}

impl EndorsementCourseState {
    pub fn new() -> Self {
        EndorsementCourseState {
            menu: MenuCore::new("Licenses and training").with_intro_help(
                "Enter books a course, or says why you do not qualify. Courses take game \
                 time, and a background check runs while you drive. Escape returns to the \
                 terminal.",
            ),
        }
    }

    /// No live suspension and nothing serious on the recent record: the
    /// 49 CFR 380.203 shape, judged from the record the game already keeps.
    fn clean_record(ctx: &GameContext) -> bool {
        let p = profile(ctx);
        !p.driving_record.suspended(p.game_hours)
            && p.driving_record.serious_in_window(p.game_hours) == 0
    }

    fn eligibility(ctx: &GameContext, cred: &Credential) -> (bool, Vec<String>) {
        let p = profile(ctx);
        let held = p.career.endorsements();
        let pending: Vec<String> = p
            .career
            .pending_credentials
            .iter()
            .map(|pc| pc.key.clone())
            .collect();
        course_eligibility(
            cred,
            p.career.level(),
            &held,
            &pending,
            Self::clean_record(ctx),
        )
    }

    fn buy(&mut self, ctx: &mut GameContext, key: &'static str) {
        let cred = credential(key).expect("a ladder credential key");
        let (ok, reasons) = Self::eligibility(ctx, cred);
        if !ok {
            ctx.audio.play("ui/error");
            ctx.say(&reasons.join(" "));
            return;
        }
        if profile(ctx).money() < cred.course_cost {
            ctx.audio.play("ui/error");
            ctx.say(&format!(
                "The {} course costs {} dollars and you have {}.",
                cred.label,
                fmt_grouped(cred.course_cost, 0),
                fmt_grouped(profile(ctx).money(), 0)
            ));
            return;
        }
        // The course takes real game time, off duty at the school -- the
        // terminal-sleep shape: clock, duty log, market day, then speech.
        let (start, end) = {
            let p = profile_mut(ctx);
            p.spend(cred.course_cost);
            let start = p.game_hours;
            p.game_hours += cred.course_hours;
            (start, p.game_hours)
        };
        crate::states::city::record_city_duty(ctx, "off_duty", start, end, "credential course");
        let mut announcements: Vec<String> = Vec::new();
        // Day and night follow the school's own clock, the one the city
        // menu reads out, not the Eastern reference clock.
        let start_local = to_local(start, city_local_zone(ctx));
        let fatigue_mode_rate = tuning_for_time_scale(ctx.settings.time_scale).fatigue_rate;
        let money = {
            let p = profile_mut(ctx);
            // Off duty on the hours clock: a stretch of 10+ hours still resets
            // HOS the way consecutive off duty does. Fatigue accrues at the
            // classroom rate -- sitting in class is not sleep (GitHub #314) --
            // and a multi-day course counts only its last class day, scored
            // 8 AM to 4 PM local. Training a carrier requires is on duty (49
            // CFR 395.2), and the clock still advances a flat course_hours:
            // both ROADMAP debt.
            p.hos.off_duty(cred.course_hours * 60.0);
            let fatigue = {
                let p = &*p;
                hos::course_fatigue(p.fatigue, start_local, cred.course_hours * 60.0, |h| {
                    fatigue_mode_rate * p.fatigue_buff_rate(start + h)
                })
            };
            p.fatigue = fatigue;
            let day = p.market_day();
            p.market.advance_to(day);
            if cred.wait_days > 0.0 {
                let ready_at_h = p.game_hours + cred.wait_days * HOURS_PER_DAY;
                p.career.pending_credentials.push(PendingCredential {
                    key: cred.key.to_string(),
                    ready_at_h,
                });
            } else {
                let before = p.career.endorsements();
                p.career.purchased_endorsements.push(cred.key.to_string());
                announcements.push(cred.announcement.to_string());
                let after = p.career.endorsements();
                if after.contains("tank")
                    && after.contains("hazmat")
                    && !(before.contains("tank") && before.contains("hazmat"))
                {
                    announcements
                        .push(ff_core::models::career::X_COMBINATION_ANNOUNCEMENT.to_string());
                }
            }
            p.dispatch_board_cache = None;
            p.money()
        };
        ctx.save_profile();
        ctx.audio.play("ui/cash");
        let mut complete = if cred.wait_days > 0.0 {
            format!(
                "Course complete, application submitted: {} dollars. The background check \
                 takes about {} days and clears while you drive. You have {} dollars left.",
                fmt_grouped(cred.course_cost, 0),
                cred.wait_days as i64,
                fmt_grouped(money, 0)
            )
        } else {
            format!(
                "Course complete: {} dollars, and you earned the {}. Matching freight is \
                 unlocked. You have {} dollars left.",
                fmt_grouped(cred.course_cost, 0),
                cred.gate_label,
                fmt_grouped(money, 0)
            )
        };
        if let Some(cue) = course_fatigue_cue(profile(ctx).fatigue) {
            complete.push(' ');
            complete.push_str(cue);
        }
        ctx.say(&complete);
        for line in announcements {
            ctx.say_with(line, crate::app::Say::queued());
        }
        ctx.award_achievement("self_paid_course");
        self.refresh(ctx, true);
    }
}

/// The warning a course ends with when it leaves the driver drowsy or worse,
/// on the same thresholds the road's yawn and rumble-strip cues use. It has
/// no speech category of its own: it is appended to the course completion
/// line, so it is heard wherever that line is.
pub fn course_fatigue_cue(fatigue: f64) -> Option<&'static str> {
    if fatigue >= hos::FATIGUE_SEVERE {
        Some("You're dangerously drowsy after the course. Sleep before you drive.")
    } else if fatigue >= hos::FATIGUE_DROWSY {
        Some("You're drowsy after the course. Sleep before you drive.")
    } else {
        None
    }
}

impl Menu for EndorsementCourseState {
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
            "Licenses and training. Certificates are carrier training, free at their listed \
             levels or paid early. Endorsements and cards take a written test, a course fee, \
             and for hazmat and the port card a background check wait. You have {} dollars. \
             {current}",
            fmt_grouped(money, 0)
        ));
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let p = profile(ctx);
        let career = &p.career;
        let earned = career.endorsements();
        let now_h = p.game_hours;
        let mut items: Vec<MenuItem<Self>> = Vec::new();
        for cred in CREDENTIALS {
            let key: &'static str = cred.key;
            if earned.contains(key) {
                let how = if career.purchased_endorsements.iter().any(|k| k == key) {
                    "self-paid course"
                } else {
                    "carrier-sponsored"
                };
                items.push(
                    MenuItem::new(
                        format!("{}: earned, {how}", py_capitalize(cred.gate_label)),
                        move |_s: &mut Self, ctx| {
                            let cred = credential(key).expect("a ladder credential key");
                            ctx.say(&format!("You already hold the {}.", cred.gate_label))
                        },
                    )
                    .help(format!(
                        "Already on your license. It unlocks {}.",
                        cred.unlocks
                    )),
                );
                continue;
            }
            if let Some(pending) = career.pending_credentials.iter().find(|pc| pc.key == key) {
                let days = ((pending.ready_at_h - now_h) / HOURS_PER_DAY)
                    .ceil()
                    .max(0.0) as i64;
                items.push(
                    MenuItem::new(
                        format!(
                            "{}: background check in progress, about {days} days left",
                            py_capitalize(cred.gate_label)
                        ),
                        move |_s: &mut Self, ctx| {
                            let cred = credential(key).expect("a ladder credential key");
                            ctx.say(&format!(
                                "Your {} background check clears while you drive.",
                                cred.gate_label
                            ))
                        },
                    )
                    .help(format!(
                        "Course done, paperwork filed. The credential activates when the \
                         check clears, and unlocks {}.",
                        cred.unlocks
                    )),
                );
                continue;
            }
            items.push(
                MenuItem::new(
                    format!(
                        "{} course: {}",
                        py_capitalize(cred.gate_label),
                        course_offer_text(cred)
                    ),
                    move |s: &mut Self, ctx| s.buy(ctx, key),
                )
                .help(course_help_text(cred)),
            );
        }
        items.push(
            MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Return to the terminal menu."),
        );
        items
    }
}

impl_state_for_menu!(EndorsementCourseState);
