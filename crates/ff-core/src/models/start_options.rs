//! Grounded career-start choices and starter carrier benefits (port of
//! `freight_fate/models/start_options.py`).

use serde::Serialize;

use crate::models::career::Career;
use crate::models::career_ladder::STARTER_CARRIER_NAME;
use crate::models::carriers::{self, carrier};
use crate::models::trailers::DEFAULT_TRAILER_PROGRAMS;
use crate::pyfmt::fmt_f;

#[cfg(test)]
mod tests;

pub const START_MODE_COMPANY: &str = "company_driver";
pub const START_MODE_OWNER_OPERATOR: &str = "owner_operator";

pub const DEFAULT_START_KEY: &str = "northstar";
pub const OWNER_OPERATOR_START_KEY: &str = "roadstead_owner_operator";

/// Carrier wage knobs for company-driver settlements.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CompanyPayPlan {
    pub pay_share: f64,
    pub min_per_mile: f64,
    pub stop_pay: f64,
    pub on_time_bonus_share: f64,
}

impl CompanyPayPlan {
    pub fn summary(&self) -> String {
        format!(
            "{} percent pay share, \
             {} dollars per mile floor, \
             {} dollar stop pay, \
             {} percent on-time bonus",
            fmt_f(self.pay_share * 100.0, 0),
            fmt_f(self.min_per_mile, 2),
            fmt_f(self.stop_pay, 0),
            fmt_f(self.on_time_bonus_share * 100.0, 0),
        )
    }
}

/// Modest carrier dispatch tendencies for generated job boards.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DispatchProfile {
    pub short_haul_bias: f64,
    pub regional_bias: f64,
    pub long_haul_bias: f64,
    pub deadline_slack: f64,
}

impl Default for DispatchProfile {
    fn default() -> Self {
        DispatchProfile {
            short_haul_bias: 0.0,
            regional_bias: 0.0,
            long_haul_bias: 0.0,
            deadline_slack: 1.0,
        }
    }
}

impl DispatchProfile {
    /// `DispatchProfile()`, usable in a `const`.
    pub const BALANCED: DispatchProfile = DispatchProfile {
        short_haul_bias: 0.0,
        regional_bias: 0.0,
        long_haul_bias: 0.0,
        deadline_slack: 1.0,
    };

    pub fn summary(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.short_haul_bias != 0.0 {
            parts.push("more short training loads");
        }
        if self.regional_bias != 0.0 {
            parts.push("more same-region lanes");
        }
        if self.long_haul_bias != 0.0 {
            parts.push("more longer lanes");
        }
        if self.deadline_slack > 1.0 {
            parts.push("more appointment slack");
        }
        if parts.is_empty() {
            "balanced dispatch".to_string()
        } else {
            parts.join(", ")
        }
    }
}

/// One career start. A static catalogue entry (never read back from a save);
/// `carrier_key` on the profile names it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CareerStartOption {
    pub key: &'static str,
    pub label: &'static str,
    pub carrier_name: &'static str,
    pub mode: &'static str,
    pub menu_summary: &'static str,
    pub help_text: &'static str,
    pub default_city: &'static str,
    pub starting_money: f64,
    pub starting_truck: &'static str,
    pub owned_trucks: &'static [&'static str],
    /// `None` means a full tank for whatever truck this start hands over,
    /// read from that model's own specs. A literal number here would drift
    /// the day a tank capacity changes and quietly start the career short of
    /// fuel.
    pub truck_fuel_gal: Option<f64>,
    pub truck_damage_pct: f64,
    pub starting_level_xp: f64,
    pub starting_deliveries: i64,
    pub starting_on_time_deliveries: i64,
    pub starting_total_miles: f64,
    pub starting_total_earnings: f64,
    pub starting_reputation: f64,
    pub company_pay: Option<CompanyPayPlan>,
    pub cargo_weight_bonus: &'static [(&'static str, f64)],
    pub dispatch: DispatchProfile,
}

impl CareerStartOption {
    pub fn is_owner_operator(&self) -> bool {
        self.mode == START_MODE_OWNER_OPERATOR
    }

    pub fn is_company_driver(&self) -> bool {
        self.mode == START_MODE_COMPANY
    }

    /// `option.cargo_weight_bonus.get(cargo_key, 0.0)`.
    /// Company carriers read bonuses from `data/carriers.json`.
    pub fn cargo_weight_bonus_for(&self, cargo_key: &str) -> f64 {
        if let Some(c) = carrier(self.key) {
            return c.cargo_weight_bonus_for(cargo_key);
        }
        self.cargo_weight_bonus
            .iter()
            .find(|(k, _)| *k == cargo_key)
            .map(|(_, bonus)| *bonus)
            .unwrap_or(0.0)
    }

    /// Dispatch biases: company carriers from `data/carriers.json`.
    pub fn dispatch_profile(&self) -> DispatchProfile {
        carrier(self.key)
            .map(|c| c.dispatch)
            .unwrap_or(self.dispatch)
    }

    /// Company pay plan from `data/carriers.json` when present.
    pub fn pay_plan(&self) -> Option<CompanyPayPlan> {
        if self.is_owner_operator() {
            return None;
        }
        carrier(self.key)
            .map(|c| c.company_pay)
            .or(self.company_pay)
    }
}

/// The dataclass defaults, for the fields an entry does not override.
const DEFAULTS: CareerStartOption = CareerStartOption {
    key: "",
    label: "",
    carrier_name: "",
    mode: START_MODE_COMPANY,
    menu_summary: "",
    help_text: "",
    default_city: "Chicago",
    starting_money: 5_000.0,
    starting_truck: "rig",
    owned_trucks: &[],
    truck_fuel_gal: None,
    truck_damage_pct: 0.0,
    starting_level_xp: 0.0,
    starting_deliveries: 0,
    starting_on_time_deliveries: 0,
    starting_total_miles: 0.0,
    starting_total_earnings: 0.0,
    starting_reputation: 50.0,
    company_pay: None,
    cargo_weight_bonus: &[],
    dispatch: DispatchProfile::BALANCED,
};

pub const NORTHSTAR_PAY: CompanyPayPlan = CompanyPayPlan {
    pay_share: 0.36,
    min_per_mile: 0.82,
    stop_pay: 175.0,
    on_time_bonus_share: 0.04,
};

/// `START_OPTIONS`, in the Python dict's order. Look entries up with
/// [`start_option`].
pub const START_OPTIONS: [CareerStartOption; 5] = [
    CareerStartOption {
        key: DEFAULT_START_KEY,
        label: "Northstar Freight Lines: balanced company driver",
        carrier_name: STARTER_CARRIER_NAME,
        mode: START_MODE_COMPANY,
        menu_summary: "Balanced company-driver start with steady wages, normal training \
                       support, and assigned carrier equipment.",
        help_text: "A balanced company-driver path. The carrier assigns and maintains \
                    the tractor, pays fuel and routine repairs, and offers steady wage \
                    math without a sharp specialty.",
        default_city: "Chicago",
        company_pay: Some(NORTHSTAR_PAY),
        ..DEFAULTS
    },
    CareerStartOption {
        key: "great_lakes_training",
        label: "Great Lakes Training Transport: trainer-friendly company driver",
        carrier_name: "Great Lakes Training Transport",
        mode: START_MODE_COMPANY,
        menu_summary: "Trainer-friendly company start with stronger stop pay, more short \
                       rookie loads, and a little more appointment slack.",
        help_text: "A practical training-fleet start. Stop pay is better on short \
                    loads, and dispatch leans toward shorter training work with a \
                    little more deadline room. Equipment and routine costs stay \
                    carrier-paid. At level 4 with full dispatch trust, Business \
                    status lets you move to another carrier.",
        default_city: "Milwaukee",
        company_pay: Some(CompanyPayPlan {
            pay_share: 0.33,
            min_per_mile: 0.74,
            stop_pay: 225.0,
            on_time_bonus_share: 0.02,
        }),
        dispatch: DispatchProfile {
            short_haul_bias: 0.8,
            deadline_slack: 1.08,
            ..DispatchProfile::BALANCED
        },
        ..DEFAULTS
    },
    CareerStartOption {
        key: "prairie_link",
        label: "Prairie Link Regional: mile-focused company driver",
        carrier_name: "Prairie Link Regional",
        mode: START_MODE_COMPANY,
        menu_summary: "Regional carrier with a better per-mile floor, lower stop pay, \
                       and more same-region grain and bulk lanes.",
        help_text: "A mile-focused company start. The per-mile wage floor is higher, \
                    but stop pay is lower, so it favors steady regional mileage over \
                    very short hops. Dispatch leans toward same-region grain and \
                    bulk work. The carrier still assigns and maintains the tractor.",
        default_city: "Kansas City",
        company_pay: Some(CompanyPayPlan {
            pay_share: 0.34,
            min_per_mile: 0.95,
            stop_pay: 130.0,
            on_time_bonus_share: 0.03,
        }),
        cargo_weight_bonus: &[("grain", 0.25), ("farm_inputs", 0.2), ("bulk", 0.15)],
        dispatch: DispatchProfile {
            regional_bias: 0.7,
            long_haul_bias: 0.1,
            ..DispatchProfile::BALANCED
        },
        ..DEFAULTS
    },
    CareerStartOption {
        key: "summit_value",
        label: "Summit Value Logistics: appointment-bonus company driver",
        carrier_name: "Summit Value Logistics",
        mode: START_MODE_COMPANY,
        menu_summary: "Higher percentage and on-time bonus for careful freight, with a \
                       smaller wage floor and more long-haul/high-value lanes.",
        help_text: "A performance-sensitive company start. Good on-time runs pay \
                    better, but the guaranteed floor is smaller. Dispatch leans \
                    toward longer and higher-value lanes. The carrier still supplies \
                    equipment, authority, insurance, fuel, and repairs.",
        default_city: "Denver",
        company_pay: Some(CompanyPayPlan {
            pay_share: 0.38,
            min_per_mile: 0.78,
            stop_pay: 150.0,
            on_time_bonus_share: 0.06,
        }),
        cargo_weight_bonus: &[("electronics", 0.2), ("automotive", 0.15), ("parcel", 0.15)],
        dispatch: DispatchProfile {
            long_haul_bias: 0.35,
            ..DispatchProfile::BALANCED
        },
        ..DEFAULTS
    },
    CareerStartOption {
        key: OWNER_OPERATOR_START_KEY,
        label: "Owner-operator start: higher risk, higher responsibility",
        carrier_name: "Northstar Freight Lines",
        mode: START_MODE_OWNER_OPERATOR,
        menu_summary: "Leased-on owner-operator from day one: a brand-new truck of your \
                       own, and every operating cost is yours.",
        help_text: "The hardest way to begin. You start leased on with a brand-new \
                    truck you have just bought: full tank, no damage, nothing worn. \
                    Working capital is limited, and the operating costs of fuel, \
                    repairs, reserves, and settlement fees come out of your own \
                    cash instead of the carrier's. You still start at level one and \
                    climb the same career as everyone else: this changes who pays, \
                    not how far along you are.",
        default_city: "Chicago",
        // The career itself starts at zero. This option is about ECONOMICS --
        // your truck, your costs -- and never about skipping the ladder. It
        // used to grant level 18 with 35 deliveries, 42,000 miles and 70,000
        // dollars of lifetime earnings, which handed the player most of a
        // thirty-level arc and published a career history that never happened
        // on their public profile.
        //
        // The truck itself is a design change of its own (owner, 2026-08-11):
        // it used to open with 110 gallons and 4 percent damage, which read as
        // a hand-me-down. Buying in means buying NEW, so the condition record
        // is left pristine and the tank is filled from the model's own specs.
        // The difficulty stays where it belongs -- in the costs and the thin
        // cushion -- rather than in a truck that starts already worn.
        starting_money: 18_000.0,
        owned_trucks: &["rig"],
        dispatch: DispatchProfile {
            long_haul_bias: 0.25,
            ..DispatchProfile::BALANCED
        },
        ..DEFAULTS
    },
];

/// Start options for the catalog carriers without a hand-written entry in
/// [`START_OPTIONS`] (the slice 3 regionals and locals), built once from
/// `data/carriers.json`, in catalog order. Their pay, dispatch and cargo
/// knobs come from the carrier record like every company start's; the words
/// say only what the game does today: the run-band maximum and the lane
/// area. The band minimum is not enforced yet and home time is inert, so
/// neither is promised.
fn generated_start_options() -> &'static [CareerStartOption] {
    static GENERATED: std::sync::OnceLock<Vec<CareerStartOption>> = std::sync::OnceLock::new();
    GENERATED.get_or_init(|| {
        let world = crate::data::world::get_world();
        carriers::carrier_catalog()
            .values()
            .filter(|c| !START_OPTIONS.iter().any(|o| o.key == c.key))
            .map(|c| {
                let leak = |text: String| -> &'static str { Box::leak(text.into_boxed_str()) };
                let tier = if c.is_local() { "local" } else { "regional" };
                let area = if c.lane_area.is_empty() {
                    "its home region".to_string()
                } else {
                    c.lane_area.clone()
                };
                let max = fmt_miles(c.run_band.max_mi);
                let default_city = c
                    .terminal_city_keys
                    .first()
                    .and_then(|k| world.cities.get(k))
                    .map(|city| city.name.clone())
                    .unwrap_or_else(|| "Chicago".to_string());
                CareerStartOption {
                    key: leak(c.key.clone()),
                    label: leak(format!("{}: {tier} company driver, {area}", c.name)),
                    carrier_name: leak(c.name.clone()),
                    mode: START_MODE_COMPANY,
                    menu_summary: leak(format!(
                        "A {tier} carrier in {area}, with loads of up to {max} miles."
                    )),
                    help_text: leak(format!(
                        "A {tier} company-driver start in {area}. Dispatch offers loads of \
                         up to {max} miles. The carrier assigns and maintains the tractor \
                         and pays fuel and routine repairs."
                    )),
                    default_city: leak(default_city),
                    company_pay: Some(c.company_pay),
                    dispatch: c.dispatch,
                    ..DEFAULTS
                }
            })
            .collect()
    })
}

fn fmt_miles(miles: f64) -> String {
    format!("{}", miles.round() as i64)
}

/// `START_OPTIONS.get(key or DEFAULT_START_KEY, START_OPTIONS[DEFAULT_START_KEY])`,
/// over every start option (see [`all_start_options`]).
pub fn start_option(key: Option<&str>) -> &'static CareerStartOption {
    let key = match key {
        Some("") | None => DEFAULT_START_KEY,
        Some(k) => k,
    };
    all_start_options()
        .into_iter()
        .find(|option| option.key == key)
        .unwrap_or_else(|| default_start_option())
}

fn default_start_option() -> &'static CareerStartOption {
    START_OPTIONS
        .iter()
        .find(|option| option.key == DEFAULT_START_KEY)
        .expect("the default start is in START_OPTIONS")
}

pub fn company_start_options() -> Vec<&'static CareerStartOption> {
    all_start_options()
        .into_iter()
        .filter(|option| option.is_company_driver())
        .collect()
}

/// Every start option in menu order: the hand-written company starts, then
/// one per remaining catalog carrier, then the owner-operator start last.
pub fn all_start_options() -> Vec<&'static CareerStartOption> {
    let (company, owner): (Vec<_>, Vec<_>) = START_OPTIONS
        .iter()
        .partition(|option| option.is_company_driver());
    company
        .into_iter()
        .chain(generated_start_options().iter())
        .chain(owner)
        .collect()
}

pub fn pay_plan_for_key(key: Option<&str>) -> CompanyPayPlan {
    let key = match key {
        Some("") | None => DEFAULT_START_KEY,
        Some(k) => k,
    };
    carriers::pay_plan_for_carrier(key)
}

/// What `apply_start_option` writes on a freshly created or reset `Profile`.
/// Each method is one attribute assignment or method call the Python made,
/// so `Profile` implements this by delegating to its own fields.
// TODO(lead): implement for models::profile::Profile (wave 2).
pub trait StartProfile {
    fn set_carrier_key(&mut self, key: &str);
    fn set_home_terminal_city(&mut self, city: &str);
    /// The home base city the driver picked (a new career's current city).
    fn home_base_city(&self) -> String;
    /// Where the truck starts: the hiring carrier's terminal city.
    fn set_current_city(&mut self, city: &str);
    /// The facility the truck starts parked at (the carrier terminal).
    fn set_parked_facility(&mut self, facility: &str);
    /// The picked home city (the driver's home, not the terminal city).
    fn set_home_city(&mut self, city: &str);
    fn set_start_mode(&mut self, mode: &str);
    fn set_carrier_name(&mut self, name: &str);
    fn set_money(&mut self, money: f64);
    fn set_business_status(&mut self, status: &str);
    fn set_truck(&mut self, key: &str);
    fn set_owned_trucks(&mut self, trucks: Vec<String>);
    fn set_owned_trailers(&mut self, trailers: Vec<String>);
    fn set_trailer_programs(&mut self, programs: Vec<String>);
    /// `profile.upgrades = {}`.
    fn clear_upgrades(&mut self);
    /// `profile.truck_conditions = {}`.
    fn clear_truck_conditions(&mut self);
    /// `profile.active_truck_key()`.
    fn active_truck_key(&self) -> String;
    /// `profile.owned_trucks`.
    fn owned_trucks(&self) -> Vec<String>;
    /// `profile.provision_truck_condition(key)`.
    fn provision_truck_condition(&mut self, key: &str);
    /// `profile.truck_specs().fuel_tank_gal`.
    fn truck_fuel_tank_gal(&self) -> f64;
    /// `profile.truck_fuel_gal = ...` (the active truck's record).
    fn set_truck_fuel_gal(&mut self, gallons: f64);
    /// `profile.truck_damage_pct = ...` (the active truck's record).
    fn set_truck_damage_pct(&mut self, pct: f64);
    /// `profile.active_trip = None`.
    fn clear_active_trip(&mut self);
    /// `profile.dispatch_board_cache = None`.
    fn clear_dispatch_board_cache(&mut self);
    fn set_pay_advance(&mut self, amount: f64);
    fn set_pay_advance_used_for_load(&mut self, used: bool);
    /// `profile.career`, for the starting career numbers.
    fn career_mut(&mut self) -> &mut Career;
}

/// Give every truck this start hands over a brand-new condition record.
///
/// A condition record carries nine dimensions -- fuel, damage, tire, brake
/// and engine wear, grime, tire compound, whether chains are aboard, and
/// chain wear -- and a start option only ever named two of them. Rebuilding
/// the record instead of poking fields means a new dimension arrives
/// pristine by default rather than silently starting a fresh career already
/// worn. Any condition a start option deliberately wants worn is applied
/// afterwards.
fn provision_start_trucks<P: StartProfile + ?Sized>(profile: &mut P, option: &CareerStartOption) {
    profile.clear_truck_conditions();
    // `{profile.active_truck_key(), *profile.owned_trucks}`: a set, so each
    // key is provisioned once; the order does not matter to the result.
    let mut keys = vec![profile.active_truck_key()];
    for key in profile.owned_trucks() {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    for key in &keys {
        profile.provision_truck_condition(key);
    }
    if let Some(fuel) = option.truck_fuel_gal {
        // Never above the real tank: the option's number is a starting level,
        // not a license to overfill a truck whose capacity has since changed.
        profile.set_truck_fuel_gal(fuel.min(profile.truck_fuel_tank_gal()));
    }
    if option.truck_damage_pct != 0.0 {
        profile.set_truck_damage_pct(option.truck_damage_pct);
    }
}

/// The carrier a start option hires with (a leased owner-operator start maps
/// to the carrier it leases to).
pub fn hiring_carrier_for_option(option: &CareerStartOption) -> Option<&'static carriers::Carrier> {
    carriers::hiring_carrier(
        option.key,
        option.carrier_name,
        if option.is_owner_operator() {
            "leased_owner_operator"
        } else {
            "company_driver"
        },
    )
}

/// Start options whose carrier hires in `city_key`, in menu order: nationals
/// across the lower 48, regionals only within their hiring radius.
pub fn start_options_for_home_city(
    world: &crate::data::world::World,
    city_key: &str,
) -> Vec<&'static CareerStartOption> {
    all_start_options()
        .into_iter()
        .filter(|option| {
            hiring_carrier_for_option(option).is_some_and(|c| c.hires_in(world, city_key))
        })
        .collect()
}

/// How many distinct carriers hire in `city_key`. The owner-operator start
/// leases to a carrier already listed, so it is not another carrier.
pub fn hiring_carrier_count_for_home_city(
    world: &crate::data::world::World,
    city_key: &str,
) -> usize {
    let mut keys: Vec<&str> = start_options_for_home_city(world, city_key)
        .into_iter()
        .filter_map(|option| hiring_carrier_for_option(option).map(|c| c.key.as_str()))
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys.len()
}

/// Apply a start option to a freshly created or reset profile.
pub fn apply_start_option<P: StartProfile + ?Sized>(profile: &mut P, option: &CareerStartOption) {
    profile.set_carrier_key(option.key);
    profile.set_start_mode(option.mode);
    profile.set_carrier_name(option.carrier_name);
    // Home terminal: the hiring carrier's terminal city nearest the picked
    // home base (a carrier-owned yard, never a world freight pin).
    let world = crate::data::world::get_world();
    let base = profile.home_base_city();
    let base = if base.trim().is_empty() {
        option.default_city.to_string()
    } else {
        base
    };
    let hiring = hiring_carrier_for_option(option);
    // The start picker offers only carriers that hire in the picked city, so
    // this is the carrier's own terminal in range. A start never falls back
    // to another carrier's far-away terminal: with no home from this carrier
    // (independent, or a regional out of range) the picked city stays and the
    // truck is not moved.
    let carrier_home = carriers::home_terminal_city_for(hiring, world, &base);
    let home = carrier_home
        .clone()
        .unwrap_or_else(|| world.resolve_city_key(&base));
    profile.set_home_terminal_city(&home);
    profile.set_home_city(&world.resolve_city_key(&base));
    // New hires do orientation and truck assignment at the carrier terminal,
    // so the truck starts there, not in the picked home city.
    if let (Some(carrier), Some(home)) = (hiring, carrier_home.as_deref()) {
        profile.set_current_city(home);
        profile.set_parked_facility(&carrier.terminal_name(world, home).unwrap_or_default());
    }
    profile.set_money(option.starting_money);
    profile.set_business_status(if option.is_owner_operator() {
        "leased_owner_operator"
    } else {
        "company_driver"
    });
    profile.set_truck(option.starting_truck);
    profile.set_owned_trucks(option.owned_trucks.iter().map(|k| k.to_string()).collect());
    profile.set_owned_trailers(Vec::new());
    profile.set_trailer_programs(if option.is_owner_operator() {
        DEFAULT_TRAILER_PROGRAMS
            .iter()
            .map(|k| k.to_string())
            .collect()
    } else {
        Vec::new()
    });
    profile.clear_upgrades();
    provision_start_trucks(profile, option);
    profile.clear_active_trip();
    profile.clear_dispatch_board_cache();
    profile.set_pay_advance(0.0);
    profile.set_pay_advance_used_for_load(false);
    let career = profile.career_mut();
    career.xp = option.starting_level_xp;
    career.deliveries = option.starting_deliveries;
    career.on_time_deliveries = option.starting_on_time_deliveries;
    career.total_miles = option.starting_total_miles;
    career.total_earnings = option.starting_total_earnings;
    career.reputation = option.starting_reputation;
}

/// `option_for_profile(profile)` on the two fields it reads:
/// `profile.carrier_key` and `profile.carrier_name`.
pub fn option_for_carrier(carrier_key: &str, carrier_name: &str) -> &'static CareerStartOption {
    let option = start_option(Some(carrier_key));
    if option.key != DEFAULT_START_KEY || carrier_name.is_empty() {
        return option;
    }
    for candidate in all_start_options() {
        if candidate.carrier_name == carrier_name {
            return candidate;
        }
    }
    option
}

/// The start a reset gives a driver with no carrier, and the city it is
/// based on. There is no carrier to carry over, so this is the start picker's
/// rule: company starts whose carrier hires in the city, nearest hiring
/// terminal first. `cities` are tried in order (the saved home city, else the
/// current city, then the old terminal city); a city no carrier hires in is
/// skipped, so a start never goes to a carrier that cannot hire there and an
/// Alaska driver never lands at a lower-48 terminal. `None` when no listed
/// city has a hiring carrier.
pub fn reset_start_for_no_carrier(
    world: &crate::data::world::World,
    cities: &[&str],
) -> Option<(&'static CareerStartOption, String)> {
    cities
        .iter()
        .filter(|city| !city.trim().is_empty())
        .find_map(|city| {
            let key = world.resolve_city_key(city);
            start_options_for_home_city(world, &key)
                .into_iter()
                .filter(|option| option.is_company_driver())
                .filter_map(|option| {
                    let mi = hiring_carrier_for_option(option)?.hiring_distance_mi(world, &key)?;
                    Some((mi, option))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, option)| (option, key))
        })
}

/// The start option behind a profile: its `carrier_key`, or -- for a save
/// from before carrier keys -- the option whose carrier name matches.
pub fn option_for_profile<P: crate::models::enforcement::StandingProfile + ?Sized>(
    profile: &P,
) -> &'static CareerStartOption {
    option_for_carrier(profile.carrier_key(), profile.carrier_name())
}
