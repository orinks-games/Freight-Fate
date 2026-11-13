//! Consumable buffs sold at route stops (port of `freight_fate/data/buffs.py`).
//!
//! A buff is a purchase that slows how fast fatigue or rig wear accrues for a
//! while -- endurance bought before a hard leg, stacking naturally with rest
//! instead of replacing it. Food and drink also give a small instant fatigue
//! lift so a meal feels like a meal. Two hard rules from the design
//! (docs/1.9-buff-system-design.md): buffs never touch the hours-of-service
//! duty clock, and one buff is active per group -- the newest replaces its
//! predecessor, so three coffees do not stack.
//!
//! The catalog lives in buffs.json next to the data so balance passes are
//! data edits, and future systems (mini-games, passes) can grant buffs by id
//! without a parallel reward system. Availability is brand-keyed through
//! `amenities::classify_brand` -- the Iron Skillet dinner is a Petro thing,
//! showers are the Pilot/Flying J thing (free with fuel, like real life) --
//! or keyed to a stop's listed actions for generic items like the energy
//! drink. All `label`/`help`/`purchased`/`worn_off` strings are
//! player-facing speech: plain language, no jargon.

use indexmap::IndexMap;
use once_cell::sync::OnceCell;
use serde::Deserialize;

use super::amenities::classify_brand;
use super::data_resources::read_data_text;
use super::world_models::DataError;
use super::world_parsing::py_repr_str;
use crate::pyfmt::py_str_float;

/// "fatigue" buffs are timed (fixed game hours); "engine" and "tire" buffs
/// are rig services that last the rest of the trip and die with it.
pub const BUFF_GROUPS: &[&str] = &["fatigue", "engine", "tire"];

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Buff {
    pub id: String,
    pub label: String,
    pub group: String,
    pub price: f64,
    pub stop_minutes: f64,
    /// accrual multiplier for the group's axis (0..1]
    pub rate: f64,
    /// one-time fatigue relief on purchase
    pub fatigue_instant: f64,
    /// timed buffs: how long the rate holds
    pub duration_game_h: f64,
    /// rig buffs: holds until the trip ends
    pub trip_scoped: bool,
    /// amenities brand keys that sell it
    pub brands: Vec<String>,
    /// stop actions that sell it (e.g. fuel)
    pub actions: Vec<String>,
    /// free after fueling this visit (showers)
    pub free_with_fuel: bool,
    /// coffee and energy drinks: the instant lift is a loan, owed back when
    /// the buff wears off (`sim::stimulant`)
    pub stimulant: bool,
    pub help: String,
    pub purchased: String,
    pub worn_off: String,
}

impl Buff {
    /// The line spoken as a timed buff wears off on the road: its own
    /// `worn_off`, or "The energy drink has worn off." from its label.
    pub fn worn_off_line(&self) -> String {
        if self.worn_off.is_empty() {
            format!("The {} has worn off.", self.label.to_lowercase())
        } else {
            self.worn_off.clone()
        }
    }
}

#[derive(Deserialize)]
struct RawBuff {
    label: String,
    group: String,
    price: f64,
    stop_minutes: f64,
    rate: f64,
    #[serde(default)]
    fatigue_instant: f64,
    #[serde(default)]
    duration_game_h: f64,
    #[serde(default)]
    trip_scoped: bool,
    #[serde(default)]
    brands: Vec<String>,
    #[serde(default)]
    actions: Vec<String>,
    #[serde(default)]
    free_with_fuel: bool,
    #[serde(default)]
    stimulant: bool,
    #[serde(default)]
    help: String,
    #[serde(default)]
    purchased: String,
    #[serde(default)]
    worn_off: String,
}

/// Parse and validate a catalog from its JSON text (catalog order is menu
/// order, so the map keeps file order).
pub fn parse_catalog(text: &str) -> Result<IndexMap<String, Buff>, DataError> {
    let raw: IndexMap<String, RawBuff> =
        serde_json::from_str(text).map_err(|e| DataError::io(format!("buffs.json: {e}")))?;
    let mut catalog = IndexMap::new();
    for (buff_id, entry) in raw {
        let buff = Buff {
            id: buff_id.clone(),
            label: entry.label,
            group: entry.group,
            price: entry.price,
            stop_minutes: entry.stop_minutes,
            rate: entry.rate,
            fatigue_instant: entry.fatigue_instant,
            duration_game_h: entry.duration_game_h,
            trip_scoped: entry.trip_scoped,
            brands: entry.brands,
            actions: entry.actions,
            free_with_fuel: entry.free_with_fuel,
            stimulant: entry.stimulant,
            help: entry.help,
            purchased: entry.purchased,
            worn_off: entry.worn_off,
        };
        if !BUFF_GROUPS.contains(&buff.group.as_str()) {
            return Err(DataError::value(format!(
                "buff {buff_id}: unknown group {}",
                py_repr_str(&buff.group)
            )));
        }
        if !(0.0 < buff.rate && buff.rate <= 1.0) {
            return Err(DataError::value(format!(
                "buff {buff_id}: rate must be in (0, 1], got {}",
                py_str_float(buff.rate)
            )));
        }
        if buff.group == "fatigue" && buff.duration_game_h <= 0.0 {
            return Err(DataError::value(format!(
                "buff {buff_id}: fatigue buffs need duration_game_h"
            )));
        }
        if buff.group != "fatigue" && !buff.trip_scoped {
            return Err(DataError::value(format!(
                "buff {buff_id}: rig buffs must be trip_scoped"
            )));
        }
        if buff.brands.is_empty() && buff.actions.is_empty() {
            return Err(DataError::value(format!(
                "buff {buff_id}: no availability (brands or actions)"
            )));
        }
        if buff.stimulant && buff.group != "fatigue" {
            return Err(DataError::value(format!(
                "buff {buff_id}: only fatigue buffs can be stimulants"
            )));
        }
        if buff.help.is_empty() || buff.purchased.is_empty() {
            return Err(DataError::value(format!(
                "buff {buff_id}: help and purchased speech are required"
            )));
        }
        catalog.insert(buff_id, buff);
    }
    Ok(catalog)
}

fn load_catalog() -> Result<IndexMap<String, Buff>, DataError> {
    let text = read_data_text("buffs.json")
        .ok_or_else(|| DataError::io("buffs.json is missing from this build"))?;
    parse_catalog(&text)
}

static CATALOG: OnceCell<IndexMap<String, Buff>> = OnceCell::new();

/// The catalog (Python's import-time `BUFF_CATALOG`), loaded on first use;
/// an invalid catalog is fatal, as the import error was.
pub fn buff_catalog() -> &'static IndexMap<String, Buff> {
    CATALOG.get_or_init(|| load_catalog().expect("buffs.json loads and validates"))
}

/// A buff by id.
pub fn buff(id: &str) -> Option<&'static Buff> {
    buff_catalog().get(id)
}

/// The buffs a stop sells, from its brand and its listed actions.
///
/// Catalog order is menu order. Generic stops sell only action-keyed
/// items; brand signatures (Iron Skillet, showers, lube bays) appear
/// only under their brand.
pub fn buffs_for_stop(name: &str, actions: &[&str]) -> Vec<&'static Buff> {
    let brand = classify_brand(name);
    buff_catalog()
        .values()
        .filter(|buff| {
            let by_brand = brand.is_some_and(|b| buff.brands.iter().any(|key| key == b.key));
            let by_action = actions.iter().any(|a| buff.actions.iter().any(|x| x == a));
            by_brand || by_action
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! The pure parts of `tests/test_buffs.py` (catalog and availability); the
    //! purchase, expiry and save round-trip cases need Profile, TruckState and
    //! the rest-stop state and belong with those modules.
    use super::*;

    fn buff_ids(name: &str, actions: &[&str]) -> std::collections::BTreeSet<String> {
        buffs_for_stop(name, actions)
            .into_iter()
            .map(|b| b.id.clone())
            .collect()
    }

    fn set(ids: &[&str]) -> std::collections::BTreeSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_catalog_loads_and_is_valid() {
        // the loader raises on any invalid entry at import; spot-check the shape
        let catalog = buff_catalog();
        assert!(!catalog.is_empty());
        for buff in catalog.values() {
            assert!(BUFF_GROUPS.contains(&buff.group.as_str()));
            assert!(0.0 < buff.rate && buff.rate <= 1.0);
            assert!(!buff.help.is_empty() && !buff.purchased.is_empty());
            if buff.group == "fatigue" {
                assert!(buff.duration_game_h > 0.0);
                assert!(!buff.worn_off.is_empty());
            } else {
                assert!(buff.trip_scoped);
            }
        }
    }

    #[test]
    fn test_brand_availability_matches_signatures() {
        assert_eq!(
            buff_ids("Love's Travel Stop", &["fuel"]),
            set(&[
                "energy_drink",
                "diesel_additive",
                "quick_lube",
                "tire_rotation"
            ])
        );
        assert_eq!(
            buff_ids("Pilot Travel Center", &["fuel"]),
            set(&["energy_drink", "diesel_additive", "shower"])
        );
        assert_eq!(
            buff_ids("Petro Stopping Center", &["fuel", "food"]),
            set(&[
                "energy_drink",
                "diesel_additive",
                "diner_meal",
                "iron_skillet_dinner"
            ])
        );
        assert_eq!(
            buff_ids("Cactus Flats Truck Stop", &["food"]),
            set(&["diner_meal"])
        );
        assert_eq!(
            buff_ids("Big Buck's Travel Center", &[]),
            set(&["big_bucks_brisket"])
        );
        assert_eq!(
            buff_ids("Wall Drug", &["park", "save", "break", "sleep"]),
            set(&["wall_drug_five_cent_coffee", "wall_drug_free_ice_water"])
        );
        // Park-only actions alone must not invent generic fuel/food cooler items.
        assert_eq!(
            buff_ids(
                "Cactus Flats Truck Stop",
                &["park", "save", "break", "sleep"]
            ),
            set(&[])
        );
        // Other brands do not sell Wall Drug staples.
        assert!(!buff_ids("Pilot Travel Center", &["fuel"]).contains("wall_drug_five_cent_coffee"));
        assert!(!buff_ids("Pilot Travel Center", &["fuel"]).contains("wall_drug_free_ice_water"));
        assert!(!buff_ids("Big Buck's Travel Center", &[]).contains("wall_drug_five_cent_coffee"));
    }

    #[test]
    fn test_wall_drug_buffs_are_weaker_than_energy_drink_and_meals() {
        let catalog = buff_catalog();
        let coffee = &catalog["wall_drug_five_cent_coffee"];
        let water = &catalog["wall_drug_free_ice_water"];
        let energy = &catalog["energy_drink"];
        let meal = &catalog["diner_meal"];
        assert!((coffee.price - 0.05).abs() < 1e-9);
        assert_eq!(water.price, 0.0);
        // Higher rate = weaker slowdown; coffee < energy drink < meal on strength.
        assert!(coffee.rate > energy.rate);
        assert!(energy.rate > meal.rate);
        assert!(water.rate >= coffee.rate);
        assert!(coffee.fatigue_instant < energy.fatigue_instant);
        assert!(energy.fatigue_instant < meal.fatigue_instant);
        assert!(water.fatigue_instant <= coffee.fatigue_instant);
        assert!(coffee.duration_game_h < energy.duration_game_h);
        assert!(energy.duration_game_h < meal.duration_game_h);
        assert!(water.duration_game_h <= coffee.duration_game_h);
    }

    #[test]
    fn coffee_and_energy_drinks_are_the_stimulants() {
        let stimulants: std::collections::BTreeSet<&str> = buff_catalog()
            .values()
            .filter(|buff| buff.stimulant)
            .map(|buff| buff.id.as_str())
            .collect();
        assert_eq!(
            stimulants,
            ["energy_drink", "wall_drug_five_cent_coffee"]
                .into_iter()
                .collect()
        );
    }

    #[test]
    fn an_invalid_catalog_is_refused_with_the_python_message() {
        let err = parse_catalog(
            r#"{"x": {"label": "X", "group": "nap", "price": 1, "stop_minutes": 1, "rate": 0.5}}"#,
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "buff x: unknown group 'nap'");
        let err = parse_catalog(
            r#"{"x": {"label": "X", "group": "fatigue", "price": 1, "stop_minutes": 1, "rate": 1.5}}"#,
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "buff x: rate must be in (0, 1], got 1.5");
    }
}
