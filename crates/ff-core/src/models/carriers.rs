//! Company carriers loaded from `data/carriers.json` (Career 2.0 carrier scope).
//!
//! Start-flow UX (`START_OPTIONS`) still names the career opens; pay knobs,
//! dispatch biases, cargo bonuses, tier, terminals, hiring radius, and run
//! band live here. `home_time_policy` is stored but inert until the week/home
//! loop lands — never claim weekly home time in UI or speech.

use indexmap::IndexMap;
use once_cell::sync::OnceCell;
use serde::Deserialize;

use crate::data::data_resources::read_data_text;
use crate::data::world::{air_miles, get_world, World};
use crate::data::world_models::{City, DataError, HomeTerminal};
use crate::models::start_options::{CompanyPayPlan, DispatchProfile, NORTHSTAR_PAY};

pub const CARRIER_TIERS: &[&str] = &["local", "regional", "national"];

#[derive(Debug, Clone, PartialEq)]
pub struct RunBand {
    pub min_mi: f64,
    pub max_mi: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Carrier {
    pub key: String,
    pub name: String,
    pub tier: String,
    pub terminal_city_keys: Vec<String>,
    /// `None` = nationwide (national tier).
    pub hiring_radius_mi: Option<f64>,
    /// Stored for later week/home work; must stay inert in speech/UI.
    pub home_time_policy: String,
    pub run_band: RunBand,
    pub company_pay: CompanyPayPlan,
    pub dispatch: DispatchProfile,
    pub cargo_weight_bonus: Vec<(String, f64)>,
    pub solvency_fallback: bool,
    /// Spoken lane area for the start picker ("the Pacific Northwest").
    /// Empty for carriers with a hand-written start option.
    pub lane_area: String,
}

impl Carrier {
    pub fn cargo_weight_bonus_for(&self, cargo_key: &str) -> f64 {
        self.cargo_weight_bonus
            .iter()
            .find(|(k, _)| k == cargo_key)
            .map(|(_, bonus)| *bonus)
            .unwrap_or(0.0)
    }

    /// Effective board distance cap: tighter of the level cap and this
    /// carrier's run-band max.
    pub fn effective_distance_cap(&self, level_cap_mi: f64) -> f64 {
        level_cap_mi.min(self.run_band.max_mi)
    }

    pub fn run_band_allows(&self, miles: f64) -> bool {
        miles + f64::EPSILON >= self.run_band.min_mi && miles <= self.run_band.max_mi + f64::EPSILON
    }

    pub fn is_national(&self) -> bool {
        self.tier == "national"
    }

    pub fn is_local(&self) -> bool {
        self.tier == "local"
    }

    /// Air miles from `city_key` to the terminal this carrier would hire a
    /// driver living there into; `None` when it does not hire there.
    pub fn hiring_distance_mi(&self, world: &World, city_key: &str) -> Option<f64> {
        let key = world.resolve_city_key(city_key);
        let origin = world.cities.get(&key)?;
        let terminal = self.hiring_terminal_city(world, &key)?;
        let t = world.cities.get(&terminal)?;
        Some(air_miles(origin.lat, origin.lon, t.lat, t.lon))
    }

    /// The carrier's own dispatch yard in `city_key`: "{Carrier} {City}
    /// terminal". Only cities in `terminal_city_keys` have one.
    pub fn terminal_name(&self, world: &World, city_key: &str) -> Option<String> {
        let key = world.resolve_city_key(city_key);
        if !self.terminal_city_keys.contains(&key) {
            return None;
        }
        let city = world.cities.get(&key)?;
        Some(format!("{} {} terminal", self.name, city.name))
    }

    /// Announceable home terminal for `city_key`, when it is one of this
    /// carrier's terminal cities.
    pub fn home_terminal(&self, world: &World, city_key: &str) -> Option<HomeTerminal> {
        let key = world.resolve_city_key(city_key);
        let name = self.terminal_name(world, &key)?;
        let city = world.cities.get(&key)?;
        Some(HomeTerminal::new(
            &name,
            &city.name,
            &city.state,
            CARRIER_TERMINAL_KIND,
        ))
    }

    /// Terminal city nearest to `city_key` (air miles), regardless of the
    /// hiring area or tier. Homes go through [`Carrier::home_terminal_city`],
    /// which keeps regionals inside their hiring radius.
    pub fn nearest_terminal_city(&self, world: &World, city_key: &str) -> Option<String> {
        let key = world.resolve_city_key(city_key);
        let origin = world.cities.get(&key)?;
        self.terminals_by_distance(world, origin)
            .into_iter()
            .next()
            .map(|(_, k)| k)
    }

    /// Terminal city this carrier would hire a driver living in `city_key`
    /// into: nearest terminal for nationals (lower 48 only), nearest terminal
    /// within `hiring_radius_mi` for regional and local carriers (same
    /// country as that terminal). `None` means the carrier does not hire
    /// there.
    pub fn hiring_terminal_city(&self, world: &World, city_key: &str) -> Option<String> {
        let key = world.resolve_city_key(city_key);
        let origin = world.cities.get(&key)?;
        if self.is_national() {
            if !in_lower_48(origin) {
                return None;
            }
            return self
                .terminals_by_distance(world, origin)
                .into_iter()
                .next()
                .map(|(_, k)| k);
        }
        let radius = self.hiring_radius_mi?;
        self.terminals_by_distance(world, origin)
            .into_iter()
            .find(|(miles, terminal_key)| {
                *miles <= radius + f64::EPSILON
                    && world
                        .cities
                        .get(terminal_key)
                        .is_some_and(|t| same_country(t, origin))
            })
            .map(|(_, k)| k)
    }

    /// Terminal city this carrier homes a career based near `city_key` in:
    /// a national's nearest terminal (all are in the lower 48, so an existing
    /// Alaska career maps there), a regional's nearest terminal only within
    /// its hiring radius. `None` means this carrier has no home for that
    /// city; the caller falls back to a carrier that does, never to a
    /// regional terminal out of range.
    pub fn home_terminal_city(&self, world: &World, city_key: &str) -> Option<String> {
        if self.is_national() {
            self.nearest_terminal_city(world, city_key)
        } else {
            self.hiring_terminal_city(world, city_key)
        }
    }

    pub fn hires_in(&self, world: &World, city_key: &str) -> bool {
        self.hiring_terminal_city(world, city_key).is_some()
    }

    fn terminals_by_distance(&self, world: &World, origin: &City) -> Vec<(f64, String)> {
        let mut out: Vec<(f64, String)> = self
            .terminal_city_keys
            .iter()
            .filter_map(|k| {
                let t = world.cities.get(k)?;
                Some((air_miles(origin.lat, origin.lon, t.lat, t.lon), k.clone()))
            })
            .collect();
        sort_nearest_first(&mut out);
        out
    }
}

/// Nearest first; an exact tie goes to the lower city key, so the answer
/// never depends on the order terminals are listed in `carriers.json`.
fn sort_nearest_first(terminals: &mut [(f64, String)]) {
    terminals.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
}

/// `HomeTerminal::kind` for a carrier-owned terminal (spoken by name only).
pub const CARRIER_TERMINAL_KIND: &str = "carrier_terminal";

/// National carriers hire in the lower 48 only: Alaska waits for an Alaska
/// regional, Hawaii has no road network, and BC/YT are not US hiring areas.
fn in_lower_48(city: &City) -> bool {
    city.country.eq_ignore_ascii_case("us")
        && !matches!(city.state_code.to_ascii_uppercase().as_str(), "AK" | "HI")
}

fn same_country(a: &City, b: &City) -> bool {
    a.country.eq_ignore_ascii_case(&b.country)
}

#[derive(Deserialize)]
struct RawRunBand {
    min: f64,
    max: f64,
}

#[derive(Deserialize)]
struct RawPay {
    pay_share: f64,
    min_per_mile: f64,
    stop_pay: f64,
    on_time_bonus_share: f64,
}

#[derive(Deserialize)]
struct RawDispatch {
    #[serde(default)]
    short_haul_bias: f64,
    #[serde(default)]
    regional_bias: f64,
    #[serde(default)]
    long_haul_bias: f64,
    #[serde(default = "one")]
    deadline_slack: f64,
}

fn one() -> f64 {
    1.0
}

#[derive(Deserialize)]
struct RawCarrier {
    name: String,
    tier: String,
    terminal_city_keys: Vec<String>,
    #[serde(default)]
    hiring_radius_mi: Option<f64>,
    #[serde(default = "inert_policy")]
    home_time_policy: String,
    run_band_mi: RawRunBand,
    company_pay: RawPay,
    #[serde(default)]
    dispatch: RawDispatch,
    #[serde(default)]
    cargo_weight_bonus: IndexMap<String, f64>,
    #[serde(default)]
    solvency_fallback: bool,
    #[serde(default)]
    lane_area: String,
}

fn inert_policy() -> String {
    "inert".into()
}

impl Default for RawDispatch {
    fn default() -> Self {
        RawDispatch {
            short_haul_bias: 0.0,
            regional_bias: 0.0,
            long_haul_bias: 0.0,
            deadline_slack: 1.0,
        }
    }
}

fn parse_catalog(text: &str) -> Result<IndexMap<String, Carrier>, DataError> {
    let raw: IndexMap<String, RawCarrier> = serde_json::from_str(text)
        .map_err(|err| DataError::value(format!("carriers.json: {err}")))?;
    let mut catalog = IndexMap::new();
    let mut fallbacks = 0usize;
    for (key, raw) in raw {
        if key.trim().is_empty() {
            return Err(DataError::value("carriers.json: empty carrier key"));
        }
        if !CARRIER_TIERS.contains(&raw.tier.as_str()) {
            return Err(DataError::value(format!(
                "carriers.json: carrier {key} has unknown tier {}",
                raw.tier
            )));
        }
        if raw.terminal_city_keys.is_empty() {
            return Err(DataError::value(format!(
                "carriers.json: carrier {key} needs at least one terminal city"
            )));
        }
        if raw.run_band_mi.min <= 0.0 || raw.run_band_mi.max < raw.run_band_mi.min {
            return Err(DataError::value(format!(
                "carriers.json: carrier {key} has invalid run_band_mi"
            )));
        }
        if raw.tier == "national" && raw.hiring_radius_mi.is_some() {
            return Err(DataError::value(format!(
                "carriers.json: national carrier {key} must leave hiring_radius_mi null"
            )));
        }
        if raw.tier != "national" && raw.hiring_radius_mi.is_none() {
            return Err(DataError::value(format!(
                "carriers.json: {tier} carrier {key} needs hiring_radius_mi",
                tier = raw.tier
            )));
        }
        if raw.solvency_fallback {
            fallbacks += 1;
        }
        let carrier = Carrier {
            key: key.clone(),
            name: raw.name,
            tier: raw.tier,
            terminal_city_keys: raw.terminal_city_keys,
            hiring_radius_mi: raw.hiring_radius_mi,
            home_time_policy: raw.home_time_policy,
            run_band: RunBand {
                min_mi: raw.run_band_mi.min,
                max_mi: raw.run_band_mi.max,
            },
            company_pay: CompanyPayPlan {
                pay_share: raw.company_pay.pay_share,
                min_per_mile: raw.company_pay.min_per_mile,
                stop_pay: raw.company_pay.stop_pay,
                on_time_bonus_share: raw.company_pay.on_time_bonus_share,
            },
            dispatch: DispatchProfile {
                short_haul_bias: raw.dispatch.short_haul_bias,
                regional_bias: raw.dispatch.regional_bias,
                long_haul_bias: raw.dispatch.long_haul_bias,
                deadline_slack: raw.dispatch.deadline_slack,
            },
            cargo_weight_bonus: raw.cargo_weight_bonus.into_iter().collect(),
            solvency_fallback: raw.solvency_fallback,
            lane_area: raw.lane_area,
        };
        catalog.insert(key, carrier);
    }
    if fallbacks != 1 {
        return Err(DataError::value(format!(
            "carriers.json: exactly one solvency_fallback carrier required, found {fallbacks}"
        )));
    }
    Ok(catalog)
}

fn load_catalog() -> Result<IndexMap<String, Carrier>, DataError> {
    let text = read_data_text("carriers.json")
        .ok_or_else(|| DataError::io("carriers.json is missing from this build"))?;
    parse_catalog(&text)
}

static CATALOG: OnceCell<IndexMap<String, Carrier>> = OnceCell::new();

pub fn carrier_catalog() -> &'static IndexMap<String, Carrier> {
    CATALOG.get_or_init(|| load_catalog().expect("carriers.json loads and validates"))
}

pub fn carrier(key: &str) -> Option<&'static Carrier> {
    carrier_catalog().get(key)
}

pub fn solvency_fallback_carrier() -> &'static Carrier {
    carrier_catalog()
        .values()
        .find(|c| c.solvency_fallback)
        .expect("carriers.json names exactly one solvency fallback")
}

/// Validate that every `terminal_city_keys` entry is a world city key.
///
/// Carrier terminals belong to the carrier ("{Carrier} {City} terminal");
/// world `terminal`/`company_yard` pins are freight endpoints and are not
/// required (or used) here.
pub fn validate_carrier_terminals() -> Result<(), DataError> {
    validate_terminals_against(carrier_catalog().values(), get_world())
}

fn validate_terminals_against<'a>(
    carriers: impl IntoIterator<Item = &'a Carrier>,
    world: &World,
) -> Result<(), DataError> {
    for carrier in carriers {
        for city_key in &carrier.terminal_city_keys {
            if !world.cities.contains_key(city_key) {
                return Err(DataError::value(format!(
                    "carriers.json: carrier {} terminal city {city_key} is not a world city key",
                    carrier.key
                )));
            }
        }
    }
    Ok(())
}

/// The carrier a profile works for: its catalog key, or (for a leased
/// owner-operator whose start key is not a carrier) the carrier it is leased
/// to by name. Independent authority has no hiring carrier.
pub fn hiring_carrier(
    carrier_key: &str,
    carrier_name: &str,
    business_status: &str,
) -> Option<&'static Carrier> {
    if business_status == crate::models::business::INDEPENDENT_AUTHORITY {
        return None;
    }
    carrier(carrier_key).or_else(|| {
        let name = carrier_name.trim();
        if name.is_empty() {
            return None;
        }
        carrier_catalog().values().find(|c| c.name == name)
    })
}

/// A home base is offerable only when some carrier hires there.
pub fn is_offerable_home_city(world: &World, city_key: &str) -> bool {
    // A road endpoint with no freight (Whittier, Bear Valley) has no work.
    let key = world.resolve_city_key(city_key);
    if world.cities.get(&key).is_some_and(|city| city.no_freight) {
        return false;
    }
    carrier_catalog()
        .values()
        .any(|c| c.hires_in(world, city_key))
}

/// Home terminal city for a career based near `city_key`: the hiring
/// carrier's terminal city from [`Carrier::home_terminal_city`]. `None` when
/// there is no hiring carrier, or a regional's hiring radius does not reach
/// the city (Prairie Link has no home for Seattle or Surrey, BC). Callers
/// fall back to a carrier that hires there (the start picker offers only
/// those; a termination moves the driver to the national last-chance
/// carrier), never to an out-of-range regional terminal.
pub fn home_terminal_city_for(
    carrier: Option<&Carrier>,
    world: &World,
    city_key: &str,
) -> Option<String> {
    carrier?.home_terminal_city(world, &world.resolve_city_key(city_key))
}

/// Home terminal city a career must store: the hiring carrier's (see
/// [`home_terminal_city_for`]), else, for a regional out of its hiring
/// radius, the national solvency-fallback carrier's terminal city (never an
/// Alaska, BC, or Yukon city, never an out-of-range regional terminal).
/// With no hiring carrier (own authority) the city key itself.
pub fn home_terminal_city_or_fallback(
    carrier: Option<&Carrier>,
    world: &World,
    city_key: &str,
) -> String {
    let key = world.resolve_city_key(city_key);
    if carrier.is_none() {
        return key;
    }
    home_terminal_city_for(carrier, world, &key)
        .or_else(|| solvency_fallback_carrier().home_terminal_city(world, &key))
        .unwrap_or(key)
}

/// A company driver with no carrier: let go where no other carrier hires
/// (see [`fallback_carrier_for`]), waiting at home to apply somewhere.
pub fn is_unassigned_company_driver(
    carrier_key: &str,
    carrier_name: &str,
    business_status: &str,
) -> bool {
    (business_status.is_empty() || business_status == crate::models::business::COMPANY_DRIVER)
        && carrier_key.trim().is_empty()
        && carrier_name.trim().is_empty()
}

/// The carrier that takes on a driver `firing_key` has just let go, picked
/// from the driver's home city (plan section 5):
///
/// 1. never the carrier that did the firing;
/// 2. a regional or local that hires in the home city, nearest hiring
///    terminal first (ties by carrier key);
/// 3. else the national last-chance carrier (Great Lakes Training), which
///    hires across the lower 48; other nationals do not take a driver
///    another carrier just let go;
/// 4. else `None`: no carrier hires there, and the driver stays home
///    without a seat. An Alaska driver is never sent to a lower-48 carrier,
///    because no lower-48 carrier hires in Alaska.
pub fn fallback_carrier_for(
    world: &World,
    home_city: &str,
    firing_key: &str,
) -> Option<&'static Carrier> {
    let home = world.resolve_city_key(home_city);
    let mut local: Vec<(f64, &'static Carrier)> = carrier_catalog()
        .values()
        .filter(|c| !c.is_national() && c.key != firing_key)
        .filter_map(|c| c.hiring_distance_mi(world, &home).map(|mi| (mi, c)))
        .collect();
    local.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.key.cmp(&b.1.key)));
    if let Some((_, c)) = local.first() {
        return Some(c);
    }
    let last_chance = solvency_fallback_carrier();
    (last_chance.key != firing_key && last_chance.hires_in(world, &home)).then_some(last_chance)
}

/// A carrier an unassigned driver can apply to, and where it would hire them.
#[derive(Debug, Clone, PartialEq)]
pub struct CarrierOpening {
    pub carrier: &'static Carrier,
    /// The terminal city the carrier would hire into.
    pub terminal_city: String,
    /// The driver's home city after the hire: the current home when the
    /// carrier hires there, else the terminal city (the driver moves).
    pub home_city: String,
    pub moves_home: bool,
}

/// Carriers an unassigned driver living in `home_city` can apply to, never
/// `excluded_key` (the carrier that let them go): first those that hire in
/// the home city, nearest terminal first; when there are none, those with a
/// terminal in the same state or province, which means moving there.
pub fn openings_for_unassigned(
    world: &World,
    home_city: &str,
    excluded_key: &str,
) -> Vec<CarrierOpening> {
    let home = world.resolve_city_key(home_city);
    let Some(origin) = world.cities.get(&home) else {
        return Vec::new();
    };
    let mut near: Vec<(f64, CarrierOpening)> = carrier_catalog()
        .values()
        .filter(|c| c.key != excluded_key)
        .filter_map(|c| {
            let terminal = c.hiring_terminal_city(world, &home)?;
            let mi = c.hiring_distance_mi(world, &home)?;
            Some((
                mi,
                CarrierOpening {
                    carrier: c,
                    terminal_city: terminal,
                    home_city: home.clone(),
                    moves_home: false,
                },
            ))
        })
        .collect();
    if near.is_empty() {
        for c in carrier_catalog().values().filter(|c| c.key != excluded_key) {
            let mut best: Option<(f64, String)> = None;
            for t in &c.terminal_city_keys {
                let Some(tc) = world.cities.get(t) else {
                    continue;
                };
                if !same_country(tc, origin)
                    || !tc.state_code.eq_ignore_ascii_case(&origin.state_code)
                {
                    continue;
                }
                let mi = air_miles(origin.lat, origin.lon, tc.lat, tc.lon);
                if best
                    .as_ref()
                    .is_none_or(|(b, k)| mi < *b || (mi == *b && t < k))
                {
                    best = Some((mi, t.clone()));
                }
            }
            if let Some((mi, t)) = best {
                near.push((
                    mi,
                    CarrierOpening {
                        carrier: c,
                        terminal_city: t.clone(),
                        home_city: t,
                        moves_home: true,
                    },
                ));
            }
        }
    }
    near.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then_with(|| a.1.carrier.key.cmp(&b.1.carrier.key))
    });
    near.into_iter().map(|(_, o)| o).collect()
}

/// Pay plan for a carrier key; unknown keys fall back to Northstar wages.
pub fn pay_plan_for_carrier(key: &str) -> CompanyPayPlan {
    carrier(key).map(|c| c.company_pay).unwrap_or(NORTHSTAR_PAY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::enforcement::LAST_CHANCE_CARRIER_KEY;

    #[test]
    fn test_carriers_json_loads_and_retiers() {
        let catalog = carrier_catalog();
        assert_eq!(catalog.len(), 24);
        let northstar = carrier("northstar").expect("northstar");
        assert_eq!(northstar.tier, "national");
        assert_eq!(northstar.run_band.min_mi, 400.0);
        assert_eq!(northstar.run_band.max_mi, 3000.0);
        assert!(northstar.hiring_radius_mi.is_none());

        let gl = carrier("great_lakes_training").expect("great lakes");
        assert_eq!(gl.tier, "national");
        assert!(gl.solvency_fallback);
        assert_eq!(solvency_fallback_carrier().key, LAST_CHANCE_CARRIER_KEY);

        let prairie = carrier("prairie_link").expect("prairie");
        assert_eq!(prairie.tier, "regional");
        assert_eq!(prairie.hiring_radius_mi, Some(250.0));
        assert_eq!(prairie.run_band.min_mi, 150.0);
        assert_eq!(prairie.run_band.max_mi, 600.0);
        assert_eq!(
            prairie.terminal_city_keys,
            vec![
                "kansas_city_mo_us".to_string(),
                "omaha_ne_us".to_string(),
                "wichita_ks_us".to_string(),
            ]
        );

        let summit = carrier("summit_value").expect("summit");
        assert_eq!(summit.tier, "national");
        assert_eq!(summit.home_time_policy, "inert");
    }

    #[test]
    fn test_terminal_tie_break_goes_to_the_lower_city_key() {
        let mut v = vec![
            (120.0, "wichita_ks_us".to_string()),
            (80.0, "omaha_ne_us".to_string()),
            (120.0, "kansas_city_mo_us".to_string()),
        ];
        sort_nearest_first(&mut v);
        let keys: Vec<&str> = v.iter().map(|(_, k)| k.as_str()).collect();
        assert_eq!(keys, ["omaha_ne_us", "kansas_city_mo_us", "wichita_ks_us"]);
        // The listed order of terminals never changes a carrier's answer.
        let world = get_world();
        let prairie = carrier("prairie_link").unwrap();
        let mut reversed = prairie.clone();
        reversed.terminal_city_keys.reverse();
        for city in [
            "topeka_ks_us",
            "lincoln_ne_us",
            "wichita_ks_us",
            "salina_ks_us",
        ] {
            assert_eq!(
                prairie.home_terminal_city(world, city),
                reversed.home_terminal_city(world, city),
                "{city}"
            );
        }
    }

    #[test]
    fn test_regional_out_of_range_falls_back_to_the_national_fallback_carrier() {
        let world = get_world();
        let prairie = carrier("prairie_link").unwrap();
        for city in ["seattle_wa_us", "surrey_bc_ca", "healy_ak_us"] {
            assert_eq!(prairie.home_terminal_city(world, city), None, "{city}");
            assert_eq!(
                home_terminal_city_or_fallback(Some(prairie), world, city),
                "milwaukee_wi_us",
                "{city}"
            );
        }
        assert_eq!(
            home_terminal_city_or_fallback(Some(prairie), world, "wichita_ks_us"),
            "wichita_ks_us"
        );
    }

    #[test]
    fn test_carrier_terminal_city_keys_are_world_cities() {
        validate_carrier_terminals().expect("terminals validate");
        let mut bad = carrier("prairie_link").unwrap().clone();
        bad.terminal_city_keys.push("atlantis_xx_us".into());
        let err = validate_terminals_against([&bad], get_world()).expect_err("unknown city");
        assert!(err.to_string().contains("atlantis_xx_us"), "{err}");
    }

    #[test]
    fn test_carrier_terminal_is_named_for_carrier_and_city() {
        let world = get_world();
        let northstar = carrier("northstar").unwrap();
        let t = northstar
            .home_terminal(world, "chicago_il_us")
            .expect("chicago");
        assert_eq!(t.name, "Northstar Freight Lines Chicago terminal");
        assert_eq!(t.kind, CARRIER_TERMINAL_KIND);
        assert_eq!(t.spoken_name(), "Northstar Freight Lines Chicago terminal");
        // Not a terminal city for this carrier: no yard is invented.
        assert!(northstar.home_terminal(world, "milwaukee_wi_us").is_none());
        let prairie = carrier("prairie_link").unwrap();
        assert_eq!(
            prairie.terminal_name(world, "omaha_ne_us").as_deref(),
            Some("Prairie Link Regional Omaha terminal")
        );
    }

    #[test]
    fn test_home_base_offerability_follows_carrier_hiring() {
        let world = get_world();
        // Chicago: Northstar hires nationally and has its terminal there.
        assert!(is_offerable_home_city(world, "chicago_il_us"));
        let northstar = carrier("northstar").unwrap();
        assert_eq!(
            northstar
                .hiring_terminal_city(world, "chicago_il_us")
                .as_deref(),
            Some("chicago_il_us")
        );
        // Alaska: no national hires there, but the Alaska regional and the
        // Anchorage local do.
        assert!(is_offerable_home_city(world, "healy_ak_us"));
        assert!(is_offerable_home_city(world, "anchorage_ak_us"));
        assert!(is_offerable_home_city(world, "fairbanks_ak_us"));
        assert!(northstar
            .hiring_terminal_city(world, "anchorage_ak_us")
            .is_none());
        // BC and YT stay blocked.
        assert!(!is_offerable_home_city(world, "whitehorse_yt_ca"));
        assert!(!is_offerable_home_city(world, "surrey_bc_ca"));
        // Prairie Link hires within 250 mi of its terminals only.
        let prairie = carrier("prairie_link").unwrap();
        assert_eq!(
            prairie
                .hiring_terminal_city(world, "topeka_ks_us")
                .as_deref(),
            Some("kansas_city_mo_us")
        );
        assert!(prairie
            .hiring_terminal_city(world, "seattle_wa_us")
            .is_none());
        // A national hires anywhere in the lower 48, into its nearest terminal.
        assert_eq!(
            northstar
                .hiring_terminal_city(world, "seattle_wa_us")
                .as_deref(),
            Some("chicago_il_us")
        );
    }

    #[test]
    fn test_hiring_carrier_maps_leases_and_skips_independents() {
        use crate::models::business::{COMPANY_DRIVER, INDEPENDENT_AUTHORITY};
        assert_eq!(
            hiring_carrier("prairie_link", "", COMPANY_DRIVER).map(|c| c.key.as_str()),
            Some("prairie_link")
        );
        assert_eq!(
            hiring_carrier(
                crate::models::start_options::OWNER_OPERATOR_START_KEY,
                "Northstar Freight Lines",
                "leased_owner_operator",
            )
            .map(|c| c.key.as_str()),
            Some("northstar")
        );
        assert!(hiring_carrier(
            "northstar",
            "Northstar Freight Lines",
            INDEPENDENT_AUTHORITY
        )
        .is_none());
    }

    #[test]
    fn test_effective_distance_cap_takes_the_tighter_limit() {
        let prairie = carrier("prairie_link").unwrap();
        assert_eq!(prairie.effective_distance_cap(1200.0), 600.0);
        assert_eq!(prairie.effective_distance_cap(400.0), 400.0);
        assert!(prairie.run_band_allows(150.0));
        assert!(prairie.run_band_allows(600.0));
        assert!(!prairie.run_band_allows(149.0));
        assert!(!prairie.run_band_allows(601.0));
    }
}
