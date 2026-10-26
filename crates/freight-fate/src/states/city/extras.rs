//! The terminal's small side menus: picking a nearby city to bobtail to,
//! paying down what a driver owes, and applying to a carrier when the
//! driver has none.

use ff_core::models::carriers::{openings_for_unassigned, CarrierOpening};
use ff_core::models::jobs::make_reposition_job;
use ff_core::models::solvency;

use crate::app::GameContext;
use crate::impl_state_for_menu;
use crate::meaningful_play::MeaningfulPlayReason;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::city::{
    launch_driving, profile, profile_mut, CityMenuState, DrivingLaunch, LaunchAnnouncement,
    DRIVE_PHASE_DELIVERY,
};

/// Pick a nearby city to bobtail (drive empty) to, to shop its board.
pub struct BobtailDestState {
    menu: MenuCore<Self>,
    cities: Vec<String>,
}

impl BobtailDestState {
    pub fn new(cities: Vec<String>) -> Self {
        BobtailDestState {
            menu: MenuCore::new("Bobtail to a nearby city").with_intro_help(
                "Drive empty to a nearby city for its dispatch board. No load and no pay, costs \
                 fuel and hours of service. Escape returns to the terminal.",
            ),
            cities,
        }
    }

    fn start(&mut self, ctx: &mut GameContext, dest: &str) {
        if let Some(line) = crate::states::city::cdl_drive_refusal(ctx) {
            ctx.audio.play("ui/error");
            ctx.say(&line);
            return;
        }
        let world = ctx.world;
        let (job, route) = {
            let p = profile(ctx);
            (
                make_reposition_job(world, &p.current_city, dest, false, None),
                world
                    .supported_route(&p.current_city, dest, None)
                    .ok()
                    .flatten(),
            )
        };
        let (Some(job), Some(route)) = (job, route) else {
            ctx.audio.play("ui/error");
            ctx.say("No route to that city.");
            return;
        };
        profile_mut(ctx).dispatch_board_cache = None;
        let spoken_dest = job.spoken_destination().to_string();
        let line = format!(
            "Bobtail to {spoken_dest}, {} on {}. No pay.",
            ctx.settings.distance_text(route.miles(), false),
            route.highways().first().cloned().unwrap_or_default()
        );
        ctx.mark_meaningful_play(MeaningfulPlayReason::DriveStarted);
        launch_driving(
            ctx,
            DrivingLaunch::new(
                job,
                route,
                DRIVE_PHASE_DELIVERY,
                LaunchAnnouncement::Line(line),
            ),
        );
    }
}

impl Menu for BobtailDestState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let world = ctx.world;
        let here = profile(ctx).current_city.clone();
        let mut items: Vec<MenuItem<Self>> = Vec::new();
        for name in &self.cities {
            let miles = world
                .supported_route(&here, name, None)
                .ok()
                .flatten()
                .map(|route| route.miles())
                .unwrap_or(0.0);
            let (city_name, state) = world
                .city(name)
                .map(|c| (c.name.clone(), c.state.clone()))
                .unwrap_or_else(|_| (name.clone(), String::new()));
            let label = format!(
                "{city_name}, {state}, {} empty",
                ctx.settings.distance_text(miles, false)
            );
            let dest = name.clone();
            items.push(
                MenuItem::new(label, move |s: &mut Self, ctx| s.start(ctx, &dest)).help(format!(
                    "Drive empty to {} to see its dispatch board.",
                    world.spoken_city(name, None)
                )),
            );
        }
        items.push(MenuItem::new("Back to terminal", |s: &mut Self, ctx| {
            s.go_back(ctx)
        }));
        items
    }
}

impl_state_for_menu!(BobtailDestState);

/// Pay down what a driver owes with cash, instead of waiting on collection.
pub struct PayDebtState {
    menu: MenuCore<Self>,
}

impl Default for PayDebtState {
    fn default() -> Self {
        Self::new()
    }
}

impl PayDebtState {
    pub fn new() -> Self {
        PayDebtState {
            menu: MenuCore::new("Pay down what you owe")
                .with_intro_help("Your own cash toward the balance. Escape backs out."),
        }
    }

    fn option_label(kind: &str, amount: &str) -> String {
        match kind {
            "all" => format!("Pay it all: {amount}"),
            "half" => format!("Pay half: {amount}"),
            _ => format!("Pay what you can, keeping a 200 dollar cushion: {amount}"),
        }
    }

    pub fn pay(&mut self, ctx: &mut GameContext, amount: f64) {
        let paid = solvency::pay_out_of_pocket(profile_mut(ctx), amount);
        if paid < 0.01 {
            ctx.audio.play("ui/error");
            ctx.say("That amount is no longer payable.");
            self.refresh(ctx, true);
            return;
        }
        ctx.save_profile();
        ctx.audio.play("ui/notify");
        let (owed, money) = {
            let p = profile(ctx);
            (solvency::debt_owed(p), p.money())
        };
        if owed < 1.0 {
            // Pop first, then speak: the parent's own announce_entry also
            // interrupts, and would otherwise purge this confirmation off
            // the queue mid-sentence. Same pattern as the motel flow in
            // driving_rest_states.py.
            ctx.pop_state();
            ctx.say(&format!(
                "Paid {}. Account clear. You have {}.",
                solvency::money_text(paid),
                solvency::money_text(money)
            ));
            return;
        }
        ctx.say(&format!(
            "Paid {} toward what you owed. You have {}, {} still owed.",
            solvency::money_text(paid),
            solvency::money_text(money),
            solvency::money_text(owed)
        ));
        self.refresh(ctx, true);
    }
}

impl Menu for PayDebtState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let (owed, money) = {
            let p = profile(ctx);
            (solvency::debt_owed(p), p.money())
        };
        let current = self.current_text(ctx);
        ctx.say(&format!(
            "You owe {} and have \
             {}. {current}",
            solvency::money_text(owed),
            solvency::money_text(money)
        ));
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = solvency::out_of_pocket_options(profile(ctx))
            .into_iter()
            .map(|(kind, amount)| {
                MenuItem::new(
                    Self::option_label(kind, &solvency::money_text(amount)),
                    move |s: &mut Self, ctx| s.pay(ctx, amount),
                )
                .help("A quarter of every settlement also keeps paying it down.")
            })
            .collect();
        items.push(MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)));
        items
    }
}

impl_state_for_menu!(PayDebtState);

/// A company driver with no carrier applies to one: carriers that hire in
/// the home city first, and when none do, carriers with a terminal in the
/// same state or province (taking one of those moves home there). The
/// carrier that let the driver go is never listed.
pub struct ApplyToCarrierState {
    menu: MenuCore<Self>,
    openings: Vec<CarrierOpening>,
}

impl ApplyToCarrierState {
    pub fn new(ctx: &GameContext) -> Self {
        let p = profile(ctx);
        let openings = openings_for_unassigned(
            ctx.world,
            &p.driver_home_city(),
            &p.driving_record.let_go_by,
        );
        ApplyToCarrierState {
            menu: MenuCore::new("Apply to a carrier").with_intro_help(
                "Carriers that would take you on from your home. Choosing one puts you on \
                 their payroll at the terminal named. Escape returns to the terminal.",
            ),
            openings,
        }
    }

    /// The spoken label for one opening.
    pub fn opening_label(world: &ff_core::data::world::World, opening: &CarrierOpening) -> String {
        let city = world.spoken_city(&opening.terminal_city, None);
        let tier = if opening.carrier.is_local() {
            "local"
        } else if opening.carrier.is_national() {
            "national"
        } else {
            "regional"
        };
        let mut label = format!(
            "{}: {tier} carrier, hires into its {city} terminal",
            opening.carrier.name
        );
        if opening.moves_home {
            label.push_str(&format!(", moves your home to {city}"));
        }
        label
    }

    fn join(&mut self, ctx: &mut GameContext, index: usize) {
        let Some(opening) = self.openings.get(index).cloned() else {
            return;
        };
        let world = ctx.world;
        profile_mut(ctx).join_carrier_opening(&opening);
        ctx.mark_meaningful_play(MeaningfulPlayReason::BusinessChanged);
        ctx.save_profile();
        ctx.audio.play("ui/notify");
        let city = world.spoken_city(&opening.terminal_city, None);
        let moved = if opening.moves_home {
            format!(" Your home is now {city}.")
        } else {
            String::new()
        };
        ctx.say(&format!(
            "{} has taken you on. You start at their {city} terminal, where your assigned \
             truck is waiting.{moved} The dispatch board is open again.",
            opening.carrier.name
        ));
        ctx.pop_state_with(true, false);
        let terminal = CityMenuState::new(ctx, true);
        ctx.replace_state(terminal);
    }
}

impl Menu for ApplyToCarrierState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let world = ctx.world;
        let mut items: Vec<MenuItem<Self>> = Vec::new();
        for (i, opening) in self.openings.iter().enumerate() {
            let label = Self::opening_label(world, opening);
            let help = if opening.carrier.lane_area.is_empty() {
                "Hires across the lower 48.".to_string()
            } else {
                format!("Based in {}.", opening.carrier.lane_area)
            };
            items.push(MenuItem::new(label, move |s: &mut Self, ctx| s.join(ctx, i)).help(help));
        }
        if items.is_empty() {
            items.push(
                MenuItem::new(
                    "No carrier hires near your home or in your state right now",
                    |s: &mut Self, ctx| s.go_back(ctx),
                )
                .help("Returns to the terminal."),
            );
        }
        items.push(MenuItem::new("Back to terminal", |s: &mut Self, ctx| {
            s.go_back(ctx)
        }));
        items
    }
}

impl_state_for_menu!(ApplyToCarrierState);
