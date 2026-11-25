//! "Choose a carrier": the one no-fault choice a 1.9 career gets when its
//! carrier no longer runs nationwide in 2.0 (see `ff_core::models::carryover`).
//!
//! Staying comes first, then every other carrier that hires where the
//! driver lives, in the career start picker's order. Each row asks twice,
//! the way "Move to another carrier" does, because the choice is final.

use ff_core::models::business::is_owner_operator;
use ff_core::models::carriers::CarrierOpening;
use ff_core::models::profile::CarrierChoiceRow;
use ff_core::models::start_options::carrier_summary_in;

use crate::app::GameContext;
use crate::impl_state_for_menu;
use crate::meaningful_play::MeaningfulPlayReason;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::city::{profile, profile_mut, ApplyToCarrierState, CityMenuState};

pub struct ChooseCarrierState {
    menu: MenuCore<Self>,
    rows: Vec<CarrierChoiceRow>,
    /// The row whose first Enter was heard; the next Enter on it chooses.
    armed: Option<usize>,
}

impl ChooseCarrierState {
    pub fn new(ctx: &GameContext) -> Self {
        ChooseCarrierState {
            menu: MenuCore::new("Choose a carrier").with_intro_help(
                "Carriers that hire where you live. Choosing one puts you on their payroll at \
                 the terminal named, and it is final. Escape returns to the terminal.",
            ),
            rows: profile(ctx).carrier_choice_rows(ctx.world),
            armed: None,
        }
    }

    fn label(ctx: &GameContext, row: &CarrierChoiceRow) -> String {
        let world = ctx.world;
        let city = world.spoken_city(&row.terminal_city, None);
        if !row.stay {
            return ApplyToCarrierState::opening_label(
                world,
                &CarrierOpening {
                    carrier: row.carrier,
                    terminal_city: row.terminal_city.clone(),
                    home_city: String::new(),
                    moves_home: false,
                },
            );
        }
        let carrier = &row.carrier.name;
        if profile(ctx).carrier_still_serves_home() {
            format!("Stay with {carrier}: {city} terminal")
        } else {
            format!("Stay with {carrier}: moves your home to {city}")
        }
    }

    /// What the row's carrier hauls. Staying says the load limit and lane
    /// area the carrier keeps now, which its hand-written start summary
    /// leaves out; the other rows read their start summary, as the start
    /// picker does.
    fn summary(ctx: &GameContext, row: &CarrierChoiceRow) -> String {
        if row.stay {
            carrier_summary_in(row.carrier, &ctx.settings)
        } else {
            row.option.summary_in(&ctx.settings)
        }
    }

    fn choose(&mut self, ctx: &mut GameContext, index: usize) {
        let Some(row) = self.rows.get(index).cloned() else {
            return;
        };
        if self.armed != Some(index) {
            self.armed = Some(index);
            let summary = Self::summary(ctx, &row);
            let starts = if is_owner_operator(&profile(ctx).business_status) {
                String::new()
            } else {
                row.carrier
                    .terminal_name(ctx.world, &row.terminal_city)
                    .map(|t| format!(" Truck starts at {t}."))
                    .unwrap_or_default()
            };
            let name = &row.carrier.name;
            if ctx.touch_hints() {
                ctx.say(&format!(
                    "{name}. {summary}{starts} Double tap again to choose."
                ));
            } else {
                ctx.say(&format!(
                    "{name}. {summary}{starts} Press Enter again to choose."
                ));
            }
            return;
        }
        self.armed = None;
        let world = ctx.world;
        let line = profile_mut(ctx).choose_carrier(world, &row);
        ctx.mark_meaningful_play(MeaningfulPlayReason::BusinessChanged);
        ctx.save_profile();
        ctx.audio.play("ui/notify");
        ctx.say(&line);
        ctx.pop_state_with(true, false);
        let terminal = CityMenuState::new(ctx, true);
        ctx.replace_state(terminal);
    }
}

impl Menu for ChooseCarrierState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = Vec::new();
        for (i, row) in self.rows.iter().enumerate() {
            let label = Self::label(ctx, row);
            let help = Self::summary(ctx, row);
            items.push(MenuItem::new(label, move |s: &mut Self, ctx| s.choose(ctx, i)).help(help));
        }
        items.push(MenuItem::new("Back to terminal", |s: &mut Self, ctx| {
            s.go_back(ctx)
        }));
        items
    }
}

impl_state_for_menu!(ChooseCarrierState);
