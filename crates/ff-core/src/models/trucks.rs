//! Truck catalog and owner-operator garage upgrades (port of
//! `freight_fate/models/trucks.py`).
//!
//! Owner-operator upgrades and chosen tractors live on the player profile.
//! Company drivers use carrier-assigned equipment and do not apply owned
//! upgrades.

use std::collections::{BTreeMap, HashMap};

use indexmap::IndexMap;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

use crate::sim::vehicle::TruckSpecs;

#[cfg(test)]
mod tests;

// Upgrade effect constants.
/// +10% torque per tier.
pub const ENGINE_TUNE_TORQUE_PER_TIER: f64 = 0.10;
/// -12% drag.
pub const AERO_DRAG_MULT: f64 = 0.88;
pub const TANK_EXTRA_GAL: f64 = 50.0;
/// Fade onset pushed this much hotter.
pub const BRAKE_FADE_BONUS_C: f64 = 150.0;

// Equipment traits dispatch matches a load against. A real yard does not hand
// out tractors at random: a run that cannot be finished inside one driving
// shift needs a bunk, a heavy load needs a tractor spec'd to pull it, and a
// day's worth of city stops is day-cab work. These are the axes that decision
// turns on (see ``dispatch_policy.assignment_for_job``).
/// No bunk: local and regional turns only.
pub const CAB_DAY: &str = "day";
/// A bed, so the driver can rest on the road.
pub const CAB_SLEEPER: &str = "sleeper";
/// Tare-light, leaves payload headroom, modest pull.
pub const SPEC_LIGHT: &str = "light";
/// The everyday fleet tractor.
pub const SPEC_STANDARD: &str = "standard";
/// Heavy-spec driveline for weight and mountain work.
pub const SPEC_HEAVY: &str = "heavy";

/// One catalogue tractor. A static entry: built once into
/// [`TRUCK_CATALOG`], never read back from a save.
#[derive(Debug, Clone, PartialEq)]
pub struct TruckModel {
    pub key: &'static str,
    pub label: &'static str,
    pub price: f64,
    pub description: &'static str,
    pub specs: TruckSpecs,
    pub cab: &'static str,
    pub spec: &'static str,
}

/// One owned truck's persistent condition, keyed by catalog key on the
/// profile. Saved as `dataclasses.asdict`; unknown keys in an old record are
/// dropped on load, as `from_dict` did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TruckCondition {
    pub fuel_gal: f64,
    pub damage_pct: f64,
    pub tire_wear_pct: f64,
    pub grime_pct: f64,
}

impl Default for TruckCondition {
    fn default() -> Self {
        TruckCondition {
            fuel_gal: 150.0,
            damage_pct: 0.0,
            tire_wear_pct: 0.0,
            grime_pct: 0.0,
        }
    }
}

impl TruckCondition {
    /// A just-purchased truck: full tank for this model, everything else zero.
    pub fn fresh<U: UpgradeTiers + ?Sized>(truck_key: &str, upgrades: &U) -> Self {
        TruckCondition {
            fuel_gal: build_truck_specs(truck_key, upgrades).fuel_tank_gal,
            ..Self::default()
        }
    }

    /// `TruckCondition.from_dict(data)`: anything but a JSON object is a
    /// default record; unknown keys are ignored, known ones override.
    pub fn from_dict(data: &serde_json::Value) -> Self {
        if !data.is_object() {
            return Self::default();
        }
        serde_json::from_value(data.clone()).unwrap_or_default()
    }
}

/// `profile.upgrades` as the Python `dict[str, int]`: missing is tier 0.
pub trait UpgradeTiers {
    fn tier(&self, key: &str) -> i64;
}

impl UpgradeTiers for HashMap<String, i64> {
    fn tier(&self, key: &str) -> i64 {
        self.get(key).copied().unwrap_or(0)
    }
}

impl UpgradeTiers for BTreeMap<String, i64> {
    fn tier(&self, key: &str) -> i64 {
        self.get(key).copied().unwrap_or(0)
    }
}

impl UpgradeTiers for IndexMap<String, i64> {
    fn tier(&self, key: &str) -> i64 {
        self.get(key).copied().unwrap_or(0)
    }
}

impl UpgradeTiers for [(&str, i64)] {
    fn tier(&self, key: &str) -> i64 {
        self.iter()
            .find(|(k, _)| *k == key)
            .map(|(_, tier)| *tier)
            .unwrap_or(0)
    }
}

impl<const N: usize> UpgradeTiers for [(&str, i64); N] {
    fn tier(&self, key: &str) -> i64 {
        self[..].tier(key)
    }
}

/// `{}`: no upgrades at all.
pub const NO_UPGRADES: [(&str, i64); 0] = [];

fn model(
    key: &'static str,
    label: &'static str,
    price: f64,
    description: &'static str,
    specs: TruckSpecs,
    cab: &'static str,
    spec: &'static str,
) -> (&'static str, TruckModel) {
    (
        key,
        TruckModel {
            key,
            label,
            price,
            description,
            specs,
            cab,
            spec,
        },
    )
}

fn specs(
    max_torque_nm: f64,
    fuel_tank_gal: f64,
    drag_coefficient: f64,
    fuel_burn_factor: f64,
    mass_kg: f64,
) -> TruckSpecs {
    TruckSpecs {
        max_torque_nm,
        fuel_tank_gal,
        drag_coefficient,
        fuel_burn_factor,
        mass_kg,
        ..TruckSpecs::default()
    }
}

/// `TRUCK_CATALOG`, in the Python dict's order. Look entries up with
/// [`truck_model`].
pub static TRUCK_CATALOG: Lazy<IndexMap<&'static str, TruckModel>> = Lazy::new(|| {
    let base = TruckSpecs::default();
    IndexMap::from([
        model(
            "rig",
            "standard rig",
            0.0,
            "The dependable tractor you started with: better fuel economy and \
             a calmer highway feel than the heavy hauler.",
            TruckSpecs::default(),
            CAB_SLEEPER,
            SPEC_STANDARD,
        ),
        model(
            "heavy_hauler",
            "heavy hauler",
            52_000.0,
            "A brute for heavy loads and long stretches: a quarter more torque \
             and a two hundred gallon tank, but blunt aerodynamics and a \
             thirstier engine.",
            specs(3_000.0, 200.0, 0.75, 1.2, 37_500.0),
            CAB_SLEEPER,
            SPEC_HEAVY,
        ),
        // -- Carrier fleet and used-market tractors ----------------------------
        // Company drivers meet these as dispatch-assigned equipment (see
        // carrier_fleet); after the owner-operator buy-in the same models sell
        // here as used fleet units or retail trucks.
        //
        // Every tier carries day cabs and sleepers, and light, standard, and
        // heavy driveline specs, so dispatch has something real to choose
        // between when it matches a tractor to a load. Tanks never shrink from
        // one tier to the next (test_carrier_fleet pins it), which is why the
        // day cabs live in the two lower tiers -- the only place a small tank
        // is honest.
        //
        // -- Yard standard: the trainer-spec iron a new hire meets ------------
        model(
            "trainer_day_cab",
            "trainer day cab",
            26_000.0,
            "The tractor every new hire learns on: a plain day cab with soft \
             power, light steering, and nowhere to sleep. Forgiving, slow, and \
             impossible to be precious about.",
            specs(2_050.0, 150.0, 0.70, 0.94, 34_800.0),
            CAB_DAY,
            SPEC_LIGHT,
        ),
        model(
            "yard_mule",
            "yard mule",
            24_000.0,
            "A stubby day cab that spends most of its life shoving trailers \
             around the lot. Grunt down low, no manners above fifty, and it \
             drinks like it is being punished.",
            specs(2_450.0, 150.0, 0.78, 1.18, 35_900.0),
            CAB_DAY,
            SPEC_HEAVY,
        ),
        model(
            "hand_me_down_sleeper",
            "hand-me-down sleeper",
            31_000.0,
            "A tired flat-roof sleeper handed down the seniority list until it \
             reached you. The bunk is thin and the paint is faded, but it will \
             get you there and back.",
            specs(2_250.0, 150.0, 0.69, 1.06, 35_600.0),
            CAB_SLEEPER,
            SPEC_STANDARD,
        ),
        model(
            "plain_jane_conventional",
            "plain jane conventional",
            34_000.0,
            "No chrome, no fairings, no nonsense: a base-spec conventional \
             sleeper bought by the dozen. Nothing about it is special and \
             nothing about it breaks.",
            specs(2_350.0, 155.0, 0.67, base.fuel_burn_factor, 35_900.0),
            CAB_SLEEPER,
            SPEC_STANDARD,
        ),
        // -- Regional fleet: proven drivers, day work and overnight turns -----
        model(
            "city_shuttle",
            "city shuttle",
            36_000.0,
            "A short, light day cab built for stop-and-go: it turns inside its \
             own trailer, sips diesel in traffic, and leaves every pound it \
             saves for the freight.",
            specs(2_200.0, 150.0, 0.68, 0.90, 34_600.0),
            CAB_DAY,
            SPEC_LIGHT,
        ),
        model(
            "dock_hopper",
            "dock hopper",
            39_000.0,
            "The everyday city day cab: enough pull for a full trailer, tight \
             mirrors, and air seats that have seen a thousand docks. Home \
             every night by design.",
            specs(2_400.0, 155.0, 0.66, base.fuel_burn_factor, 35_600.0),
            CAB_DAY,
            SPEC_STANDARD,
        ),
        model(
            "short_haul_stubnose",
            "short-haul stubnose",
            44_000.0,
            "A heavy-spec day cab for the loads nobody wants on a light \
             tractor: gravel, steel, and drum mixers. All driveline, no \
             comfort, and it will pull a building off its footings.",
            TruckSpecs {
                brake_fade_temp_c: 430.0,
                ..specs(2_850.0, 155.0, 0.74, 1.14, 37_400.0)
            },
            CAB_DAY,
            SPEC_HEAVY,
        ),
        model(
            "midroof_runner",
            "mid-roof runner",
            54_000.0,
            "A light mid-roof sleeper for drivers who live on two-day lanes: \
             quick, economical, and just tall enough to stand up and change a \
             shirt.",
            specs(2_400.0, 160.0, 0.61, 0.92, 35_200.0),
            CAB_SLEEPER,
            SPEC_LIGHT,
        ),
        model(
            "farm_road_workhorse",
            "farm road workhorse",
            57_000.0,
            "Built for grain country: heavy driveline, tall gearing, and a \
             chassis that shrugs off washboard county roads. It hauls hoppers \
             in spring and complains about it all summer.",
            specs(2_800.0, 170.0, 0.71, 1.10, 37_200.0),
            CAB_SLEEPER,
            SPEC_HEAVY,
        ),
        model(
            "sunset_day_cab",
            "sunset day cab",
            38_000.0,
            "A tidy regional day cab: light, easy on diesel, and happy on \
             short lanes. No bunk, no long-haul pretensions.",
            specs(2_300.0, 150.0, 0.62, 0.95, 35_500.0),
            CAB_DAY,
            SPEC_LIGHT,
        ),
        model(
            "ridgeline_sleeper",
            "ridgeline sleeper",
            58_000.0,
            "A mid-roof regional sleeper with a little more pull and a \
             bigger tank: the workhorse most fleets hand a proven regional \
             driver.",
            specs(2_500.0, 165.0, 0.64, base.fuel_burn_factor, 36_200.0),
            CAB_SLEEPER,
            SPEC_STANDARD,
        ),
        model(
            "old_longnose",
            "old longnose",
            49_000.0,
            "A classic long-hood conventional: strong pull and a proud \
             profile, but it fights the wind and drinks for the privilege.",
            specs(2_700.0, 165.0, 0.72, 1.12, 36_800.0),
            CAB_SLEEPER,
            SPEC_HEAVY,
        ),
        // -- Long-haul fleet: sleepers with real interstate range -------------
        model(
            "long_run_midroof",
            "long run mid-roof",
            74_000.0,
            "A no-drama mid-roof built for the middle of the country: honest \
             range, honest power, and a bunk you can actually sleep a full ten \
             hours in.",
            specs(2_550.0, 185.0, 0.62, 0.96, 36_100.0),
            CAB_SLEEPER,
            SPEC_STANDARD,
        ),
        model(
            "dry_lightning",
            "dry lightning",
            86_000.0,
            "A light-spec aero sleeper that runs the desert lanes: it gives up \
             torque for tare weight and mileage, and it will out-run anything \
             in the yard on flat ground.",
            specs(2_450.0, 180.0, 0.56, 0.87, 34_900.0),
            CAB_SLEEPER,
            SPEC_LIGHT,
        ),
        model(
            "interstate_condo",
            "interstate condo",
            99_000.0,
            "A raised-roof sleeper with room to stand, cook, and live: the \
             truck drivers move into rather than drive. Heavy on comfort, \
             steady everywhere else.",
            specs(2_650.0, 205.0, 0.63, 1.02, 36_700.0),
            CAB_SLEEPER,
            SPEC_STANDARD,
        ),
        model(
            "steel_hauler",
            "steel hauler",
            97_000.0,
            "A heavy-spec flatbed tractor with a headache rack and a driveline \
             sized for coil and plate. Rides like a brick empty and like it \
             was born to it loaded.",
            TruckSpecs {
                brake_fade_temp_c: 430.0,
                ..specs(2_900.0, 195.0, 0.70, 1.09, 37_600.0)
            },
            CAB_SLEEPER,
            SPEC_HEAVY,
        ),
        model(
            "mountain_spec_hauler",
            "mountain spec hauler",
            104_000.0,
            "Spec'd for the western grades: deep gearing, a big retarder, and \
             brakes that stay cool where other trucks start smelling hot. \
             Slow up, unbothered down.",
            TruckSpecs {
                brake_fade_temp_c: 470.0,
                engine_brake_torque_nm: 2_050.0,
                ..specs(2_950.0, 200.0, 0.67, 1.07, 37_400.0)
            },
            CAB_SLEEPER,
            SPEC_HEAVY,
        ),
        model(
            "highline_sleeper",
            "highline sleeper",
            82_000.0,
            "A raised-roof long-haul sleeper with a two hundred gallon tank \
             and honest aerodynamics: built to live on the interstate for \
             days at a time.",
            specs(2_600.0, 200.0, 0.60, 0.97, 36_400.0),
            CAB_SLEEPER,
            SPEC_STANDARD,
        ),
        model(
            "big_bunk_conventional",
            "big bunk conventional",
            89_000.0,
            "A big-bunk conventional with serious torque for mountain \
             corridors and heavy long-haul freight. Comfortable, capable, \
             and a little thirsty.",
            specs(2_800.0, 210.0, 0.66, 1.05, 37_000.0),
            CAB_SLEEPER,
            SPEC_HEAVY,
        ),
        model(
            "aero_cruiser",
            "aero cruiser",
            95_000.0,
            "A slippery fleet aero tractor tuned for fuel economy: modest \
             torque, long legs, and the best mileage of the long-haul pool.",
            specs(2_500.0, 190.0, 0.55, 0.90, 35_800.0),
            CAB_SLEEPER,
            SPEC_LIGHT,
        ),
        // -- Premium fleet: what seniority gets you ---------------------------
        model(
            "cabover_revival",
            "cabover revival",
            118_000.0,
            "A modern cabover: short, light, and startlingly quick, with the \
             whole road laid out under the windshield. Every pound it saves \
             goes to the load, and it turns where nothing else will.",
            specs(2_600.0, 200.0, 0.59, 0.91, 34_700.0),
            CAB_SLEEPER,
            SPEC_LIGHT,
        ),
        model(
            "chrome_shop_special",
            "chrome shop special",
            124_000.0,
            "Every light the catalog sells and a stack of polished pipe: a \
             senior driver's reward truck. It is genuinely good, and it knows \
             exactly how good it looks.",
            specs(2_800.0, 210.0, 0.68, 1.06, 36_900.0),
            CAB_SLEEPER,
            SPEC_STANDARD,
        ),
        model(
            "deep_sleeper_custom",
            "deep sleeper custom",
            132_000.0,
            "A stretched custom sleeper with a proper bed and a wardrobe: for \
             drivers who are out for a month at a time and have stopped \
             pretending otherwise.",
            specs(2_850.0, 215.0, 0.64, 1.00, 37_100.0),
            CAB_SLEEPER,
            SPEC_STANDARD,
        ),
        model(
            "wide_glide_tourer",
            "wide glide tourer",
            136_000.0,
            "A wide-cab highway tourer built around the seat and the sound \
             system: quiet at speed, easy over a long day, and no trouble at \
             all in a crosswind.",
            specs(2_800.0, 220.0, 0.58, 0.94, 36_500.0),
            CAB_SLEEPER,
            SPEC_STANDARD,
        ),
        model(
            "granite_grade_king",
            "granite grade king",
            146_000.0,
            "A heavy-spec premium tractor for the worst grades on the map: \
             enormous torque, oversized brakes, and a retarder that holds a \
             loaded trailer down a six percent without a word.",
            TruckSpecs {
                brake_fade_temp_c: 480.0,
                engine_brake_torque_nm: 2_150.0,
                ..specs(3_050.0, 225.0, 0.65, 1.08, 37_700.0)
            },
            CAB_SLEEPER,
            SPEC_HEAVY,
        ),
        model(
            "summit_flagship",
            "summit flagship",
            128_000.0,
            "A premium flagship sleeper: big power, a two hundred twenty \
             gallon tank, and clean aerodynamics. The truck senior drivers \
             ask the shop about.",
            specs(2_900.0, 220.0, 0.57, 0.95, 36_600.0),
            CAB_SLEEPER,
            SPEC_HEAVY,
        ),
        model(
            "silver_aero",
            "silver aero",
            142_000.0,
            "A polished premium aero tractor: the slipperiest shape in the \
             catalog with plenty of pull, sipping diesel at cruise like a \
             truck half its size.",
            specs(2_750.0, 220.0, 0.52, 0.88, 36_000.0),
            CAB_SLEEPER,
            SPEC_LIGHT,
        ),
        // -- First pick of the yard: the carrier's best iron ------------------
        model(
            "midnight_flyer",
            "midnight flyer",
            176_000.0,
            "A light-spec flagship for drivers who run nights: the quietest \
             cab on the property, a huge tank, and mileage that makes a \
             dispatcher smile.",
            TruckSpecs {
                brake_fade_temp_c: 450.0,
                ..specs(2_800.0, 230.0, 0.53, 0.86, 35_400.0)
            },
            CAB_SLEEPER,
            SPEC_LIGHT,
        ),
        model(
            "owner_spec_showpiece",
            "owner spec showpiece",
            188_000.0,
            "Ordered the way an owner would order it, then handed to the \
             driver the carrier least wants to lose. Everything on it is the \
             good version.",
            TruckSpecs {
                brake_fade_temp_c: 455.0,
                ..specs(2_900.0, 235.0, 0.56, 0.92, 36_500.0)
            },
            CAB_SLEEPER,
            SPEC_STANDARD,
        ),
        model(
            "centurion_longhood",
            "centurion longhood",
            204_000.0,
            "A flagship long-hood with a hood you could land a plane on: \
             monstrous torque, brakes to match, and an unapologetic thirst. \
             The truck people photograph at the fuel island.",
            TruckSpecs {
                brake_fade_temp_c: 470.0,
                engine_brake_torque_nm: 2_100.0,
                ..specs(3_150.0, 245.0, 0.71, 1.10, 37_800.0)
            },
            CAB_SLEEPER,
            SPEC_HEAVY,
        ),
        model(
            "continental_expedition",
            "continental expedition",
            216_000.0,
            "Built to cross the continent without a scheduled stop: two \
             hundred fifty gallons, mountain-grade brakes, and enough torque \
             to make a loaded pull feel like an errand.",
            TruckSpecs {
                brake_fade_temp_c: 480.0,
                engine_brake_torque_nm: 2_200.0,
                ..specs(3_100.0, 250.0, 0.60, 1.00, 37_500.0)
            },
            CAB_SLEEPER,
            SPEC_HEAVY,
        ),
        model(
            "presidential_sleeper",
            "presidential sleeper",
            185_000.0,
            "The top of the yard: huge torque, a two hundred forty gallon \
             tank, and brakes that shrug off long mountain descents. First \
             pick goes to the carrier's best.",
            TruckSpecs {
                brake_fade_temp_c: 450.0,
                ..specs(3_100.0, 240.0, 0.58, 0.98, 37_200.0)
            },
            CAB_SLEEPER,
            SPEC_HEAVY,
        ),
        model(
            "night_flag_aero",
            "night flag aero",
            198_000.0,
            "A flagship aero sleeper for drivers who live out west: enormous \
             range, upgraded brakes, and the lowest drag on the road. It \
             turns fuel islands into scenery.",
            TruckSpecs {
                brake_fade_temp_c: 450.0,
                ..specs(2_950.0, 240.0, 0.50, 0.85, 36_200.0)
            },
            CAB_SLEEPER,
            SPEC_LIGHT,
        ),
    ])
});

/// `TRUCK_CATALOG.get(key)`.
pub fn truck_model(key: &str) -> Option<&'static TruckModel> {
    TRUCK_CATALOG.get(key)
}

/// `TRUCK_CATALOG[key]`: panics on an unknown key, as the Python `KeyError`
/// would. Use [`truck_model`] where the key may be stale.
pub fn truck_model_or_panic(key: &str) -> &'static TruckModel {
    truck_model(key).unwrap_or_else(|| panic!("{key:?} is not in TRUCK_CATALOG"))
}

/// A garage upgrade. `prices` has one entry per tier.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Upgrade {
    pub key: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub prices: &'static [f64],
}

impl Upgrade {
    pub fn max_tier(&self) -> i64 {
        self.prices.len() as i64
    }
}

/// `UPGRADE_CATALOG`, in the Python dict's order. Look entries up with
/// [`upgrade`].
pub const UPGRADE_CATALOG: &[Upgrade] = &[
    Upgrade {
        key: "engine_tune",
        label: "Engine tune",
        description: "Gives the truck more pulling power. It helps with heavy freight, \
                      hill climbs, mountain grades, and starting from a stop with a load. \
                      Buy it when heavy loads and steep routes feel sluggish.",
        prices: &[12_000.0, 26_000.0],
    },
    Upgrade {
        key: "aero_kit",
        label: "Aerodynamic kit",
        description: "Makes the truck burn less fuel at highway speed. It does not add \
                      more fuel capacity; it makes the same tank last longer. Buy it to \
                      save diesel money over long highway miles.",
        prices: &[9_000.0],
    },
    Upgrade {
        key: "long_range_tank",
        label: "Long-range tank",
        description: "Adds fifty gallons of fuel capacity. It does not make the truck more \
                      efficient; it lets you carry more fuel. Buy it for more distance \
                      between fuel stops and more route flexibility.",
        prices: &[7_500.0],
    },
    Upgrade {
        key: "reinforced_brakes",
        label: "Reinforced brakes",
        description: "Keeps braking power strong for longer when the brakes get hot. It \
                      helps on mountain descents, with heavy freight, and during emergency \
                      stops. Buy it when downhill control matters more than speed or range.",
        prices: &[6_500.0],
    },
];

/// `UPGRADE_CATALOG.get(key)`.
pub fn upgrade(key: &str) -> Option<&'static Upgrade> {
    UPGRADE_CATALOG.iter().find(|u| u.key == key)
}

/// Specs for the given truck model with the profile's upgrades applied.
pub fn build_truck_specs<U: UpgradeTiers + ?Sized>(truck_key: &str, upgrades: &U) -> TruckSpecs {
    let model = truck_model(truck_key).unwrap_or_else(|| truck_model_or_panic("rig"));
    let base = &model.specs;
    let mut specs = base.clone();
    let tier = upgrades.tier("engine_tune");
    if tier != 0 {
        specs.max_torque_nm =
            base.max_torque_nm * (1.0 + ENGINE_TUNE_TORQUE_PER_TIER * tier.min(2) as f64);
    }
    if upgrades.tier("aero_kit") != 0 {
        specs.drag_coefficient = base.drag_coefficient * AERO_DRAG_MULT;
    }
    if upgrades.tier("long_range_tank") != 0 {
        specs.fuel_tank_gal = base.fuel_tank_gal + TANK_EXTRA_GAL;
    }
    if upgrades.tier("reinforced_brakes") != 0 {
        specs.brake_fade_temp_c = base.brake_fade_temp_c + BRAKE_FADE_BONUS_C;
    }
    specs
}
