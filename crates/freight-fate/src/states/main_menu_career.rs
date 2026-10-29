//! Career start, home region, and home city menu states (port of
//! `freight_fate/states/main_menu_career.py`).

use std::collections::BTreeMap;

use ff_core::data::regions::region_label;
use ff_core::models::profile::{find_save_path, is_pre_1_9_save_file, Profile, DEFAULT_CITY};
use ff_core::models::start_options::{all_start_options, apply_start_option, start_option};

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

pub struct CareerStartState {
    menu: MenuCore<Self>,
    pub driver_name: String,
}

impl CareerStartState {
    pub fn new(driver_name: &str) -> Self {
        Self {
            menu: MenuCore::new("Career start").with_intro_help(
                "Company starts use assigned carrier equipment; the carrier pays \
                 fuel, repairs, insurance, and trailer support. The owner-operator \
                 start is higher risk: you own a brand-new truck and pay business \
                 costs from day one. Enter selects, Escape goes back.",
            ),
            driver_name: driver_name.to_string(),
        }
    }

    pub fn intro_help(&self) -> &str {
        &self.menu.intro_help
    }

    fn pick(&mut self, ctx: &mut GameContext, key: &str) {
        let option = start_option(Some(key));
        ctx.audio.play("ui/menu_select");
        ctx.push_state(HomeTerminalState::new(ctx, &self.driver_name, option.key));
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
        ctx.say("Career start.");
        let current = self.current_text(ctx);
        ctx.say_with(current, Say::queued().review(false));
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        all_start_options()
            .into_iter()
            .map(|option| {
                let key = option.key;
                MenuItem::new(
                    format!("{}. {}", option.label, option.menu_summary),
                    move |s: &mut Self, ctx| s.pick(ctx, key),
                )
                .help(option.help_text)
            })
            .collect()
    }
}

impl_state_for_menu!(CareerStartState);

/// Pick the region of the country where a brand-new career begins.
///
/// Region selection is the first of two levels: choosing a region opens a
/// [`HomeCityState`] listing only that region's cities. A short region list
/// keeps the spoken navigation manageable as the map grows toward national
/// coverage, instead of one long flat list of every city.
pub struct HomeTerminalState {
    menu: MenuCore<Self>,
    pub driver_name: String,
    pub start_key: String,
    cities_by_region: BTreeMap<String, Vec<String>>,
    regions: Vec<String>,
}

impl HomeTerminalState {
    pub fn new(ctx: &GameContext, driver_name: &str, start_key: &str) -> Self {
        let option = start_option(Some(start_key));
        let mut by_region: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for city in ctx.world.cities.values() {
            by_region
                .entry(city.region.clone())
                .or_default()
                .push(city.key.clone());
        }
        for keys in by_region.values_mut() {
            keys.sort_by_key(|k| ctx.world.cities[k].name.clone());
        }
        let mut regions: Vec<String> = by_region.keys().cloned().collect();
        regions.sort_by_key(|r| region_menu_name(r));
        // Start options may still name their default city pre-slug.
        let option_default = ctx.world.resolve_city_key(option.default_city);
        let default_city = if ctx.world.cities.contains_key(&option_default) {
            option_default
        } else {
            DEFAULT_CITY.to_string()
        };
        let default = ctx
            .world
            .cities
            .get(&default_city)
            .map(|c| c.region.clone());
        let mut menu = MenuCore::new("Home region").with_intro_help(
            "Up and Down, Home and End, or a typed letter pick a region. \
             Enter opens its cities, Escape goes back.",
        );
        if let Some(index) = default.and_then(|d| regions.iter().position(|r| *r == d)) {
            menu.index = index;
        }
        Self {
            menu,
            driver_name: driver_name.to_string(),
            start_key: start_key.to_string(),
            cities_by_region: by_region,
            regions,
        }
    }

    fn pick_region(&mut self, ctx: &mut GameContext, region: &str) {
        let cities = self
            .cities_by_region
            .get(region)
            .cloned()
            .unwrap_or_default();
        ctx.push_state(HomeCityState::new(
            ctx,
            &self.driver_name,
            &self.start_key,
            region,
            &cities,
        ));
    }
}

impl Menu for HomeTerminalState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let option = start_option(Some(&self.start_key));
        ctx.say(&format!(
            "Home region for your {} career.",
            option.carrier_name
        ));
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
                (format!("{place} (1 city)"), format!("{place}, 1 city."))
            } else {
                (
                    format!("{place} ({count} cities)"),
                    format!("{place}, {count} cities."),
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

/// Pick the home terminal city within a chosen region.
pub struct HomeCityState {
    menu: MenuCore<Self>,
    pub driver_name: String,
    pub start_key: String,
    pub region: String,
    cities: Vec<String>,
}

impl HomeCityState {
    pub fn new(
        ctx: &GameContext,
        driver_name: &str,
        start_key: &str,
        region: &str,
        city_names: &[String],
    ) -> Self {
        let cities: Vec<String> = city_names.to_vec();
        let option = start_option(Some(start_key));
        let option_default = ctx.world.resolve_city_key(option.default_city);
        let mut menu = MenuCore::new("Home terminal").with_intro_help(
            "Up and Down, Home and End, or a typed letter pick a city. Enter \
             confirms your home terminal, Escape goes back.",
        );
        if let Some(index) = cities.iter().position(|c| *c == option_default) {
            menu.index = index;
        } else if let Some(index) = cities.iter().position(|c| c == DEFAULT_CITY) {
            menu.index = index;
        }
        Self {
            menu,
            driver_name: driver_name.to_string(),
            start_key: start_key.to_string(),
            region: region.to_string(),
            cities,
        }
    }

    fn pick(&mut self, ctx: &mut GameContext, city: &str) {
        let name = self.driver_name.clone();
        // Loading over a same-named 1.9 career is a deliberate restart, but a
        // same-named career from an earlier version must never be overwritten:
        // the legacy notice just promised that save stays safe on disk for
        // 1.8, and this is the only path that could break the promise.
        if let Some(same_name) = find_save_path(&name) {
            if is_pre_1_9_save_file(&same_name) {
                ctx.audio.play("ui/error");
                ctx.say(&format!(
                    "There is already a career named {name} from an earlier \
                     version of Freight Fate. That save stays as it is, so this \
                     career needs a different driver name. Escape goes back to \
                     change it."
                ));
                return;
            }
        }
        let existing: Vec<String> = Profile::list_saves()
            .iter()
            .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().to_lowercase()))
            .collect();
        let option = start_option(Some(&self.start_key));
        let mut profile = Profile::named_in(&name, city);
        apply_start_option(&mut profile, option);
        if let Err(e) = profile.save() {
            log::error!("Could not save the profile: {e}");
        }
        ctx.profile = Some(profile);
        // Drop the whole new-career chain without re-entering any of it: each
        // revealed picker would otherwise announce itself again on the way past.
        ctx.pop_state_with(true, false); // this city picker
        ctx.pop_state_with(true, false); // region picker
        ctx.pop_state_with(true, false); // career start
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

impl Menu for HomeCityState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let region = region_menu_name(&self.region);
        ctx.say(&format!("{region} terminals."));
        let current = self.current_text(ctx);
        ctx.say_with(current, Say::queued().review(false));
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items = Vec::new();
        for key in &self.cities {
            let Some(city) = ctx.world.cities.get(key) else {
                continue;
            };
            let terminal_name = ctx
                .world
                .home_terminal(key)
                .map(|t| t.spoken_name())
                .unwrap_or_default();
            let place = city.spoken_qualified();
            let k = key.clone();
            items.push(
                MenuItem::new(place.clone(), move |s: &mut Self, ctx| s.pick(ctx, &k))
                    .help(format!("Start at {terminal_name} in {place}.")),
            );
        }
        items
    }
}

impl_state_for_menu!(HomeCityState);
