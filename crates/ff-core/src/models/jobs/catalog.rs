//! The cargo catalog and the facility / market tables (the top of `jobs.py`).

use indexmap::IndexMap;
use once_cell::sync::Lazy;

use crate::data::world_constants::FACILITY_CARGO_ROLES;
use crate::models::career::XpCargo;
use crate::models::cargo_condition::CargoFragility;
use crate::models::trailers::{equipment_text_for_cargo, TANK_CAPACITY_TONS};
use crate::sim::surge::LiquidCargo;

/// One cargo class on the catalog (`CargoType`, frozen).
#[derive(Debug, Clone, PartialEq)]
pub struct CargoType {
    pub key: &'static str,
    pub label: &'static str,
    /// base $ per mile
    pub rate_per_mile: f64,
    pub weight_tons: (f64, f64),
    /// every credential the load requires (empty = plain freight). Bulk
    /// fuel is the one that takes two: a real fuel tanker is placarded, so
    /// the fleet hires the X combination -- tank AND hazmat.
    pub credentials: &'static [&'static str],
    pub fragile: bool,
    pub min_level: i64,
    pub equipment: &'static str,
    // Liquid bulk. A tank load is the only freight that keeps moving after the
    // truck has stopped, so the physics layer needs to know, and whether the
    // shell has baffles in it decides how badly. Sanitation rules forbid
    // baffles in food-grade tanks -- the crevices cannot be washed out -- so
    // the freight that is hardest to haul is the milk, not the fuel.
    pub tank: bool,
    pub baffled: bool,
    /// Longer-combination-vehicle freight: only offered when both endpoint
    /// states are on the frozen LCV network (23 CFR 658 Appendix C).
    pub lcv_lanes: bool,
}

impl CargoType {
    const fn plain(
        key: &'static str,
        label: &'static str,
        rate_per_mile: f64,
        weight_tons: (f64, f64),
        credentials: &'static [&'static str],
    ) -> Self {
        CargoType {
            key,
            label,
            rate_per_mile,
            weight_tons,
            credentials,
            fragile: false,
            min_level: 1,
            equipment: "",
            tank: false,
            baffled: false,
            lcv_lanes: false,
        }
    }

    const fn fragile(self) -> Self {
        CargoType {
            fragile: true,
            ..self
        }
    }

    const fn min_level(self, min_level: i64) -> Self {
        CargoType { min_level, ..self }
    }

    const fn tank(self, baffled: bool) -> Self {
        CargoType {
            tank: true,
            baffled,
            ..self
        }
    }

    const fn lcv_lanes(self) -> Self {
        CargoType {
            lcv_lanes: true,
            ..self
        }
    }

    const fn equipment(self, equipment: &'static str) -> Self {
        CargoType { equipment, ..self }
    }

    /// The first required credential, or None for plain freight -- the
    /// one-credential view the XP and badge layers read.
    pub fn primary_credential(&self) -> Option<&'static str> {
        self.credentials.first().copied()
    }

    /// The credentials in `held` still missing for this cargo, in catalog
    /// order.
    pub fn missing_credentials<S: AsRef<str>>(&self, held: &[S]) -> Vec<&'static str> {
        self.credentials
            .iter()
            .filter(|needed| !held.iter().any(|h| h.as_ref() == **needed))
            .copied()
            .collect()
    }

    pub fn equipment_text(&self) -> String {
        if !self.equipment.is_empty() {
            return self.equipment.to_string();
        }
        equipment_text_for_cargo(self.key)
    }

    /// How full the tank is for a load of this weight, 0 to 1.
    pub fn fill_fraction(&self, weight_tons: f64) -> f64 {
        if !self.tank || TANK_CAPACITY_TONS <= 0.0 {
            return 0.0;
        }
        (weight_tons / TANK_CAPACITY_TONS).clamp(0.0, 1.0)
    }
}

impl XpCargo for CargoType {
    fn endorsement(&self) -> Option<&str> {
        self.primary_credential()
    }
    fn min_level(&self) -> i64 {
        self.min_level
    }
}

impl CargoFragility for CargoType {
    fn key(&self) -> &str {
        self.key
    }
    fn fragile(&self) -> bool {
        self.fragile
    }
}

impl LiquidCargo for CargoType {
    fn tank(&self) -> bool {
        self.tank
    }
    fn baffled(&self) -> bool {
        self.baffled
    }
    fn fill_fraction(&self, weight_tons: f64) -> f64 {
        CargoType::fill_fraction(self, weight_tons)
    }
}

/// `CARGO_CATALOG`, in the Python dict's insertion order.
pub static CARGO_CATALOG: Lazy<IndexMap<&'static str, CargoType>> = Lazy::new(|| {
    let entries = [
        CargoType::plain("general", "general freight", 2.10, (8.0, 20.0), &[]),
        CargoType::plain("retail", "retail goods", 2.25, (6.0, 16.0), &[]),
        CargoType::plain("parcel", "parcel freight", 2.55, (4.0, 12.0), &[]),
        CargoType::plain("container", "shipping containers", 2.40, (12.0, 24.0), &[]),
        CargoType::plain("bulk", "bulk materials", 2.30, (15.0, 25.0), &[]),
        CargoType::plain("grain", "grain", 2.20, (18.0, 25.0), &[]),
        CargoType::plain("farm_inputs", "farm inputs", 2.35, (10.0, 22.0), &[]),
        CargoType::plain(
            "construction",
            "construction materials",
            2.35,
            (14.0, 25.0),
            &[],
        ),
        // Banded lumber and paper rolls ride open decks: real carriers put a
        // driver through securement training before the first coil or unit
        // of lumber (the rules are 49 CFR 393 subpart I), and so does this
        // one -- the certificate sponsors at the same level the cargo
        // appears, so nobody who could haul it yesterday loses it.
        CargoType::plain(
            "lumber_paper",
            "lumber and paper products",
            2.45,
            (10.0, 24.0),
            &["flatbed_securement"],
        )
        .min_level(2),
        CargoType::plain("automotive", "automotive parts", 2.75, (8.0, 20.0), &[])
            .fragile()
            .min_level(2),
        CargoType::plain(
            "machinery",
            "heavy machinery",
            2.90,
            (15.0, 25.0),
            &["heavy_haul"],
        )
        .fragile(),
        // Steel is the securement class: coils get their own section of the
        // federal cargo-securement rules (49 CFR 393.120).
        CargoType::plain(
            "steel",
            "steel products",
            2.85,
            (16.0, 25.0),
            &["flatbed_securement"],
        )
        .min_level(3),
        CargoType::plain("food", "fresh food", 2.60, (8.0, 18.0), &["refrigerated"]).fragile(),
        CargoType::plain(
            "refrigerated",
            "refrigerated goods",
            2.85,
            (8.0, 18.0),
            &["refrigerated"],
        )
        .fragile(),
        CargoType::plain(
            "chemicals",
            "packaged industrial chemicals",
            3.05,
            (10.0, 22.0),
            &["high_value"],
        )
        .min_level(4),
        CargoType::plain(
            "electronics",
            "electronics",
            3.30,
            (4.0, 12.0),
            &["high_value"],
        )
        .fragile(),
        // Twin 28-foot trailers: the parcel networks' linehaul workhorse.
        // STAA doubles are legal on the National Network in every state --
        // the freeze only bites the longer combinations -- so the T
        // endorsement alone opens this freight coast to coast.
        //
        // Parcel fills a pup's space long before its weight: two pups of
        // boxes run about 6 to 15 tonnes. The set is also some 4 tonnes
        // heavier empty than the single van the board's weight clamp
        // assumes (two pup boxes and a converter dolly), so a heavier set
        // would be over 80,000 pounds on a real scale.
        CargoType::plain(
            "parcel_doubles",
            "twin-trailer parcel freight",
            3.15,
            (6.0, 15.0),
            &["doubles_triples"],
        )
        .min_level(8),
        // Placarded loads: the freight the H endorsement exists for. The
        // catalog's plain "chemicals" class stays non-placarded packaged
        // goods; this is the tanker-adjacent dry side -- drums, cylinders,
        // oxidizers -- that pays for the background check.
        CargoType::plain(
            "hazardous",
            "placarded hazardous materials",
            3.60,
            (8.0, 22.0),
            &["hazmat"],
        )
        .min_level(12),
        // Liquid bulk: the back half of the career, where the freight stops being
        // heavier and starts being harder. Both pay well over dry freight because
        // the load fights back -- it arrives at the stop bar a second after you do.
        //
        // Fuel rides in a baffled shell: the bulkheads spend the fore-and-aft wave
        // and it settles in a few cycles. It is the one to learn on. Gasoline is
        // placarded class 3 freight, so a real fuel hauler carries the X
        // combination -- tank AND hazmat -- and so does this one.
        CargoType::plain(
            "fuel_bulk",
            "bulk fuel",
            3.45,
            (11.0, 25.0),
            &["tank", "hazmat"],
        )
        .min_level(16)
        .tank(true),
        // Liquid food rides in a smooth bore, because sanitation rules forbid
        // baffles in a food-grade tank -- nobody can wash out the crevices. So the
        // gentlest cargo in the game travels in the most vicious equipment, and
        // a half-loaded milk tank is the hardest thing on the roster to stop.
        CargoType::plain(
            "liquid_food",
            "liquid food products",
            3.85,
            (9.0, 24.0),
            &["tank"],
        )
        .min_level(21)
        .tank(false),
        // Drayage boxes off the secure side of the waterfront: unescorted
        // access to an MTSA-regulated port terminal takes the TWIC card.
        // Ordinary "container" freight stays open -- domestic intermodal
        // ramps are not secure maritime areas.
        CargoType::plain(
            "port_container",
            "port containers",
            2.95,
            (12.0, 24.0),
            &["twic"],
        )
        .min_level(18)
        .equipment("intermodal container chassis"),
        // Turnpike doubles: two long trailers on the corridors whose networks
        // were frozen into law in 1991. The game keeps the combination at
        // legal GVW -- raising the weight ceiling on the frozen network is
        // the oversize-permit economy's job, on the roadmap.
        CargoType::plain(
            "turnpike_doubles",
            "turnpike doubles freight",
            3.95,
            (14.0, 25.0),
            &["lcv"],
        )
        .min_level(20)
        .lcv_lanes()
        // A turnpike double is two full-length trailers, not the parcel
        // pups the twin-trailer program is named for.
        .equipment("two 48-foot trailers"),
    ];
    entries.into_iter().map(|c| (c.key, c)).collect()
});

/// `CARGO_CATALOG[key]`, or None for an unknown class.
pub fn cargo_type(key: &str) -> Option<&'static CargoType> {
    CARGO_CATALOG.get(key)
}

/// The full spoken gate phrase for a required credential ("refrigerated
/// certificate", "hazmat endorsement"); `None` is the standard CDL.
pub fn endorsement_label(endorsement: Option<&str>) -> &'static str {
    match endorsement {
        None => "standard CDL",
        Some(key) => {
            crate::models::credentials::credential(key)
                .unwrap_or_else(|| panic!("no credential on the ladder for {key:?}"))
                .gate_label
        }
    }
}

/// "the tank vehicle endorsement and the hazmat endorsement": the missing
/// credentials as one spoken clause, in catalog order.
pub fn credentials_clause(keys: &[&str]) -> String {
    let phrases: Vec<String> = keys
        .iter()
        .map(|key| format!("the {}", endorsement_label(Some(key))))
        .collect();
    match phrases.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [head @ .., last] => format!("{} and {last}", head.join(", ")),
    }
}

/// `FACILITY_CARGO[facility_type]`: the classes a facility type ships or
/// receives (a set in Python; here ships first, then receives, deduplicated).
pub fn facility_cargo(facility_type: &str) -> Option<Vec<&'static str>> {
    let (_, ships, receives) = FACILITY_CARGO_ROLES
        .iter()
        .find(|(k, _, _)| *k == facility_type)?;
    let mut out: Vec<&'static str> = Vec::new();
    for key in ships.iter().chain(receives.iter()) {
        if !out.contains(key) {
            out.push(key);
        }
    }
    Some(out)
}

/// `FACILITY_CARGO` as the Python dict: every facility type and its classes.
pub fn facility_cargo_table() -> IndexMap<&'static str, Vec<&'static str>> {
    FACILITY_CARGO_ROLES
        .iter()
        .map(|(k, _, _)| (*k, facility_cargo(k).unwrap_or_default()))
        .collect()
}

pub const MARKET_TAG_CARGO_BONUS: &[(&str, &[&str])] = &[
    (
        "agriculture",
        &["grain", "food", "refrigerated", "farm_inputs", "bulk"],
    ),
    ("air", &["electronics", "parcel", "general"]),
    (
        "automotive",
        &["automotive", "steel", "machinery", "electronics"],
    ),
    ("border", &["retail", "container", "general", "parcel"]),
    ("chemical", &["chemicals", "bulk", "fuel_bulk", "hazardous"]),
    ("cold_chain", &["food", "refrigerated"]),
    (
        "construction",
        &["construction", "bulk", "steel", "lumber_paper"],
    ),
    ("energy", &["chemicals", "bulk", "fuel_bulk"]),
    ("food", &["food", "refrigerated", "grain", "liquid_food"]),
    (
        "industrial",
        &["steel", "machinery", "bulk", "construction"],
    ),
    (
        "intermodal",
        &["container", "general", "retail", "automotive"],
    ),
    ("lumber", &["lumber_paper", "construction"]),
    (
        "manufacturing",
        &["machinery", "electronics", "automotive", "steel"],
    ),
    ("mining", &["bulk", "construction", "machinery"]),
    ("parcel", &["parcel", "electronics", "parcel_doubles"]),
    (
        "port",
        &[
            "container",
            "bulk",
            "automotive",
            "chemicals",
            "port_container",
        ],
    ),
    ("retail", &["retail", "general", "parcel"]),
    ("river_port", &["bulk", "grain", "container"]),
    ("steel", &["steel", "machinery", "construction"]),
];

/// `MARKET_TAG_CARGO_BONUS.get(tag, set())`.
pub fn market_tag_cargo_bonus(tag: &str) -> &'static [&'static str] {
    MARKET_TAG_CARGO_BONUS
        .iter()
        .find(|(k, _)| *k == tag)
        .map(|(_, keys)| *keys)
        .unwrap_or(&[])
}

pub const FACILITY_SELECTION_WEIGHTS: &[(&str, f64)] = &[
    ("company_yard", 0.45),
    ("cross_dock", 1.15),
    ("dry_warehouse", 1.0),
    ("grocery_retail_dc", 1.05),
    ("port_terminal", 1.25),
    ("intermodal_ramp", 1.25),
    ("parcel_hub", 1.15),
    ("farm_elevator", 1.15),
    ("food_processor", 1.0),
    ("cold_storage", 1.0),
    ("automotive_plant", 0.95),
    ("chemical_petroleum_terminal", 0.85),
    ("steel_industrial", 0.95),
    ("mine_quarry", 0.8),
    ("lumber_paper", 0.9),
];
