//! Career start, home region, and home city menu states (port of
//! `freight_fate/states/main_menu_career.py`).

use std::collections::BTreeMap;

use ff_core::data::regions::region_label;
use ff_core::models::carriers::{home_terminal_city_for, is_offerable_home_city};
use ff_core::models::profile::{find_save_path, is_pre_1_9_save_file, Profile, DEFAULT_CITY};
use ff_core::models::start_options::{
    apply_start_option, hiring_carrier_count_for_home_city, hiring_carrier_for_option,
    start_option, start_options_for_home_city, CareerStartOption,
};

use crate::app::{GameContext, Say};
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::main_menu::{first_day_orientation_message, first_state_after_career_creation};

/// Region label suited to a menu item and first-letter jump.
///
/// The spoken labels read naturally as prose ("in the Great Lakes"), but a
/// list where every entry starts with "the" defeats type-ahead, so the
/// leading article is dropped for menu display.
pub fn region_menu_name(region: &str) -> String {
    let label = region_label(region)
        .map(str::to_string)
        .unwrap_or_else(|| region.replace('_', " "));
    match label.strip_prefix("the ") {
        Some(rest) => rest.to_string(),
        None => label,
    }
}

/// Choose among the carriers that hire in the picked home city.
///
/// The last step of a new career: region, then home city, then this list.
/// Only carriers that hire there are listed (nationals across the lower 48,
/// regionals within their hiring radius), and each row says where the truck
/// starts -- the carrier's nearest terminal, where new hires do orientation
/// and truck assignment.
pub struct CareerStartState {
    menu: MenuCore<Self>,
    pub driver_name: String,
    /// Resolved key of the picked home city.
    pub home_city: String,
    options: Vec<&'static CareerStartOption>,
}

impl CareerStartState {
    pub fn new(ctx: &GameContext, driver_name: &str, home_city: &str) -> Self {
        let home_city = ctx.world.resolve_city_key(home_city);
        let options = start_options_for_home_city(ctx.world, &home_city);
        Self {
            menu: MenuCore::new("Career start").with_intro_help(
                "Only carriers that hire in your home city are listed. Each one says where your truck starts: that carrier's nearest terminal. Company starts use assigned carrier equipment; the carrier pays fuel, repairs, insurance, and trailer support. The owner-operator start is higher risk: you own a brand-new truck and pay business costs from day one. Enter selects, Escape goes back.",
            ),
            driver_name: driver_name.to_string(),
            home_city,
            options,
        }
    }

    pub fn intro_help(&self) -> &str {
        &self.menu.intro_help
    }

    /// How many distinct carriers hire in this home city.
    pub fn carrier_count(&self, ctx: &GameContext) -> usize {
        hiring_carrier_count_for_home_city(ctx.world, &self.home_city)
    }

    /// Start option keys offered for this home city, in menu order.
    pub fn option_keys(&self) -> Vec<&'static str> {
        self.options.iter().map(|o| o.key).collect()
    }

    fn pick(&mut self, ctx: &mut GameContext, key: &str) {
        let name = self.driver_name.clone();
        // Loading over a same-named 1.9 career is a deliberate restart, but a
        // same-named career from an earlier version must never be overwritten:
        // the legacy notice just promised that save stays safe on disk for
        // 1.8, and this is the only path that could break the promise.
        if let Some(same_name) = find_save_path(&name) {
            if is_pre_1_9_save_file(&same_name) {
                ctx.audio.play("ui/error");
                ctx.say(&format!(
                    "There is already a career named {name} from an earlier version of Freight Fate. That save stays as it is, so this career needs a different driver name. Escape goes back to change it."
                ));
                return;
            }
        }
        ctx.audio.play("ui/menu_select");
        let existing: Vec<String> = Profile::list_saves()
            .iter()
            .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().to_lowercase()))
            .collect();
        let option = start_option(Some(key));
        // The picked home city seeds the home terminal; apply_start_option
        // then moves the truck to that carrier terminal.
        let mut profile = Profile::named_in(&name, &self.home_city);
        apply_start_option(&mut profile, option);
        if let Err(e) = profile.save() {
            log::error!("Could not save the profile: {e}");
        }
        ctx.profile = Some(profile);
        // Drop the whole new-career chain without re-entering any of it: each
        // revealed picker would otherwise announce itself again on the way past.
        ctx.pop_state_with(true, false); // this carrier picker
        ctx.pop_state_with(true, false); // city picker
        ctx.pop_state_with(true, false); // region picker
        ctx.pop_state_with(true, false); // name entry
        let loaded_over = if existing.contains(&name.to_lowercase()) {
            format!("Loaded over existing driver named {name}. ")
        } else {
            String::new()
        };
        // Welcome first, then whatever comes next -- every state announces
        // itself on entry, so speaking this after the push meant one of the two
        // lines was always cut off. Cutting the city menu's "parked at" was
        // harmless because the welcome repeats it; cutting the orinks.net offer
        // left the player being asked a question they never heard. Both states
        // built here queue their announcement behind this line.
        let message = first_day_orientation_message(ctx, &loaded_over);
        ctx.say(&message);
        let next = first_state_after_career_creation(ctx);
        ctx.push_shared(next);
    }
}

impl Menu for CareerStartState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let place = ctx.world.spoken_city(&self.home_city, Some(true));
        let count = hiring_carrier_count_for_home_city(ctx.world, &self.home_city);
        let noun = if count == 1 {
            "carrier hires"
        } else {
            "carriers hire"
        };
        ctx.say(&format!("Career start. {count} {noun} in {place}."));
        let current = self.current_text(ctx);
        ctx.say_with(current, Say::queued().review(false));
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        self.options
            .iter()
            .map(|option| {
                let key = option.key;
                let terminal = hiring_carrier_for_option(option).and_then(|c| {
                    let home = home_terminal_city_for(Some(c), ctx.world, &self.home_city)?;
                    c.terminal_name(ctx.world, &home)
                });
                let starts = terminal
                    .map(|t| format!(" Truck starts at {t}."))
                    .unwrap_or_default();
                MenuItem::new(
                    format!("{}. {}{starts}", option.label, option.menu_summary),
                    move |s: &mut Self, ctx| s.pick(ctx, key),
                )
                .help(format!("{}{starts}", option.help_text))
            })
            .collect()
    }
}

impl_state_for_menu!(CareerStartState);

/// Pick the region of the country where a brand-new career begins.
///
/// Region selection is the first of three levels: choosing a region opens a
/// [`HomeCityState`] listing only that region's offerable home cities, then
/// the carriers that hire there. Only cities where some carrier hires
/// (`is_offerable_home_city`) are listed, so Alaska, British Columbia, and
/// Yukon are not offered; a region with no offerable city is left out.
pub struct HomeTerminalState {
    menu: MenuCore<Self>,
    pub driver_name: String,
    cities_by_region: BTreeMap<String, Vec<String>>,
    regions: Vec<String>,
}

impl HomeTerminalState {
    pub fn new(ctx: &GameContext, driver_name: &str) -> Self {
        let cities_by_region = offerable_cities_by_region(ctx);
        let mut regions: Vec<String> = cities_by_region.keys().cloned().collect();
        regions.sort_by_key(|r| region_menu_name(r));
        let default = ctx.world.cities.get(DEFAULT_CITY).map(|c| c.region.clone());
        let mut menu = MenuCore::new("Home region").with_intro_help(
            "Up and Down, Home and End, or a typed letter pick a region. Enter opens its home cities, Escape goes back.",
        );
        if let Some(index) = default.and_then(|d| regions.iter().position(|r| *r == d)) {
            menu.index = index;
        }
        Self {
            menu,
            driver_name: driver_name.to_string(),
            cities_by_region,
            regions,
        }
    }

    /// Every city this picker offers, across all regions.
    pub fn offered_cities(&self) -> Vec<String> {
        self.cities_by_region.values().flatten().cloned().collect()
    }

    fn pick_region(&mut self, ctx: &mut GameContext, region: &str) {
        let cities = self
            .cities_by_region
            .get(region)
            .cloned()
            .unwrap_or_default();
        ctx.push_state(HomeCityState::new(ctx, &self.driver_name, region, &cities));
    }
}

/// Offerable home cities (some carrier hires there), grouped by region and
/// sorted by name.
pub fn offerable_cities_by_region(ctx: &GameContext) -> BTreeMap<String, Vec<String>> {
    let mut by_region: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for city in ctx.world.cities.values() {
        // Some carrier hires here, and a start option offers that carrier.
        if !is_offerable_home_city(ctx.world, &city.key)
            || start_options_for_home_city(ctx.world, &city.key).is_empty()
        {
            continue;
        }
        by_region
            .entry(city.region.clone())
            .or_default()
            .push(city.key.clone());
    }
    for keys in by_region.values_mut() {
        keys.sort_by_key(|k| ctx.world.cities[k].name.clone());
    }
    by_region
}

impl Menu for HomeTerminalState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        ctx.say("Home region for your new career.");
        let current = self.current_text(ctx);
        ctx.say_with(current, Say::queued().review(false));
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items = Vec::new();
        for region in &self.regions {
            // The region is a name; the count is words a catalog has whole.
            let place = region_menu_name(region);
            let count = self.cities_by_region[region].len();
            let (label, help) = if count == 1 {
                (
                    format!("{place} (1 city)"),
                    format!("{place}, 1 home city where a carrier is hiring."),
                )
            } else {
                (
                    format!("{place} ({count} cities)"),
                    format!("{place}, {count} home cities where a carrier is hiring."),
                )
            };
            let r = region.clone();
            items.push(
                MenuItem::new(label, move |s: &mut Self, ctx| s.pick_region(ctx, &r)).help(help),
            );
        }
        items
    }
}

impl_state_for_menu!(HomeTerminalState);

/// Pick the home city within a chosen region; the carrier comes next.
pub struct HomeCityState {
    menu: MenuCore<Self>,
    pub driver_name: String,
    pub region: String,
    cities: Vec<String>,
}

impl HomeCityState {
    pub fn new(ctx: &GameContext, driver_name: &str, region: &str, city_names: &[String]) -> Self {
        // Only offerable home cities, even when a caller passes others.
        let cities: Vec<String> = city_names
            .iter()
            .map(|c| ctx.world.resolve_city_key(c))
            .filter(|c| is_offerable_home_city(ctx.world, c))
            .collect();
        let mut menu = MenuCore::new("Home city").with_intro_help(
            "Up and Down, Home and End, or a typed letter pick your home city. Enter lists the carriers hiring there, Escape goes back.",
        );
        if let Some(index) = cities.iter().position(|c| c == DEFAULT_CITY) {
            menu.index = index;
        }
        Self {
            menu,
            driver_name: driver_name.to_string(),
            region: region.to_string(),
            cities,
        }
    }

    /// The home cities this picker lists (resolved keys).
    pub fn cities(&self) -> &[String] {
        &self.cities
    }

    fn pick(&mut self, ctx: &mut GameContext, city: &str) {
        ctx.audio.play("ui/menu_select");
        ctx.push_state(CareerStartState::new(ctx, &self.driver_name, city));
    }
}

impl Menu for HomeCityState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let region = region_menu_name(&self.region);
        ctx.say(&format!("{region} home cities."));
        let current = self.current_text(ctx);
        ctx.say_with(current, Say::queued().review(false));
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items = Vec::new();
        for key in &self.cities {
            let Some(city) = ctx.world.cities.get(key) else {
                continue;
            };
            let place = city.spoken_qualified();
            let count = hiring_carrier_count_for_home_city(ctx.world, key);
            let noun = if count == 1 {
                "carrier hires"
            } else {
                "carriers hire"
            };
            let k = key.clone();
            items.push(
                MenuItem::new(place.clone(), move |s: &mut Self, ctx| s.pick(ctx, &k))
                    .help(format!("Home in {place}. {count} {noun} here.")),
            );
        }
        items
    }
}

impl_state_for_menu!(HomeCityState);
