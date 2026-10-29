//! Trailer program, ownership, and cargo compatibility model (port of
//! `freight_fate/models/trailers.py`).
//!
//! Company drivers use carrier-provided trailers. Leased-on owner-operators use
//! carrier trailer programs. Own-authority players can buy trailers outright while
//! still keeping earlier support programs for save compatibility.

use serde::Serialize;

pub const DEFAULT_TRAILER_PROGRAMS: &[&str] = &["dry_van"];

/// One trailer program / purchasable trailer type. A static catalogue entry,
/// so it serialises but is never read back from a save.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TrailerType {
    pub key: &'static str,
    pub label: &'static str,
    pub equipment_text: &'static str,
    pub description: &'static str,
    pub lease_deposit: f64,
    pub per_mile_reserve: f64,
    pub purchase_price: f64,
    pub owned_per_mile_reserve: f64,
}

/// `TRAILER_CATALOG`, in the Python dict's order. Look entries up with
/// [`trailer_type`].
pub const TRAILER_CATALOG: &[TrailerType] = &[
    TrailerType {
        key: "dry_van",
        label: "Dry van",
        equipment_text: "dry van trailer",
        description: "Carrier trailer program for general boxed and pallet freight.",
        lease_deposit: 0.0,
        per_mile_reserve: 0.12,
        purchase_price: 42_000.0,
        owned_per_mile_reserve: 0.05,
    },
    TrailerType {
        key: "reefer",
        label: "Reefer",
        equipment_text: "refrigerated trailer",
        description: "Temperature-controlled trailer program for food and refrigerated freight.",
        lease_deposit: 8_000.0,
        per_mile_reserve: 0.18,
        purchase_price: 82_000.0,
        owned_per_mile_reserve: 0.10,
    },
    TrailerType {
        key: "flatbed",
        label: "Flatbed",
        equipment_text: "flatbed trailer",
        description:
            "Open-deck trailer program for steel, machinery, lumber, and construction freight.",
        lease_deposit: 7_000.0,
        per_mile_reserve: 0.16,
        purchase_price: 48_000.0,
        owned_per_mile_reserve: 0.07,
    },
    TrailerType {
        key: "bulk",
        label: "Bulk",
        equipment_text: "bulk or hopper trailer",
        description: "Bulk trailer program for grain, farm inputs, and loose bulk materials.",
        lease_deposit: 9_000.0,
        per_mile_reserve: 0.20,
        purchase_price: 58_000.0,
        owned_per_mile_reserve: 0.09,
    },
    // A tank is the most expensive box a driver can pull and the one the
    // carrier is fussiest about: pressure tests, internal wash-outs between
    // products, and a shell that is scrap the first time it is rolled. The
    // reserves reflect equipment that is inspected far more than a dry van.
    TrailerType {
        key: "tank",
        label: "Tank",
        equipment_text: "tank trailer",
        description:
            "Tank trailer program for liquid bulk: fuel, chemicals, and liquid food products.",
        lease_deposit: 14_000.0,
        per_mile_reserve: 0.26,
        purchase_price: 96_000.0,
        owned_per_mile_reserve: 0.13,
    },
    // Two 28-foot pups and a converter dolly: the parcel networks' linehaul
    // set. Twice the hookups, twice the walk-around, and the price of two
    // boxes -- but it is the equipment the doubles endorsement exists for.
    TrailerType {
        key: "double_van",
        label: "Twin trailers",
        equipment_text: "twin 28-foot trailers",
        description:
            "Twin-trailer program for doubles freight: two short vans and a converter dolly.",
        lease_deposit: 11_000.0,
        per_mile_reserve: 0.21,
        purchase_price: 74_000.0,
        owned_per_mile_reserve: 0.10,
    },
];

/// `TRAILER_CATALOG.get(key)`.
pub fn trailer_type(key: &str) -> Option<&'static TrailerType> {
    TRAILER_CATALOG.iter().find(|trailer| trailer.key == key)
}

/// `key in TRAILER_CATALOG`.
pub fn is_trailer_key(key: &str) -> bool {
    trailer_type(key).is_some()
}

/// How much liquid a road tank holds, in the same tonnes the job weights use.
/// A load's weight against this is how full the tank is -- which is the single
/// number that decides how hard it will surge.
pub const TANK_CAPACITY_TONS: f64 = 26.0;

pub const CARGO_TRAILER_COMPATIBILITY: &[(&str, &[&str])] = &[
    ("general", &["dry_van"]),
    ("retail", &["dry_van"]),
    ("parcel", &["dry_van"]),
    ("container", &["dry_van", "flatbed"]),
    ("bulk", &["bulk"]),
    ("grain", &["bulk"]),
    ("farm_inputs", &["dry_van", "bulk"]),
    ("construction", &["flatbed", "dry_van"]),
    ("lumber_paper", &["flatbed", "dry_van"]),
    ("automotive", &["dry_van"]),
    ("machinery", &["flatbed"]),
    ("steel", &["flatbed"]),
    ("food", &["reefer"]),
    ("refrigerated", &["reefer"]),
    ("chemicals", &["dry_van"]),
    ("electronics", &["dry_van"]),
    ("parcel_doubles", &["double_van"]),
    ("hazardous", &["dry_van"]),
    ("fuel_bulk", &["tank"]),
    ("liquid_food", &["tank"]),
    ("port_container", &["flatbed"]),
    ("turnpike_doubles", &["double_van"]),
];

pub fn trailer_keys_for_cargo(cargo_key: &str) -> &'static [&'static str] {
    CARGO_TRAILER_COMPATIBILITY
        .iter()
        .find(|(key, _)| *key == cargo_key)
        .map(|(_, keys)| *keys)
        .unwrap_or(DEFAULT_TRAILER_PROGRAMS)
}

/// Python's `", ".join(items[:-1]) + f", or {items[-1]}"` with the one- and
/// zero-item cases.
fn or_list(items: &[&str], empty: &str) -> String {
    match items {
        [] => empty.to_string(),
        [only] => (*only).to_string(),
        [head @ .., last] => format!("{}, or {}", head.join(", "), last),
    }
}

pub fn trailer_labels<I, S>(keys: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let labels: Vec<&str> = keys
        .into_iter()
        .filter_map(|key| trailer_type(key.as_ref()).map(|t| t.label))
        .collect();
    or_list(&labels, "carrier trailer")
}

pub fn equipment_text_for_cargo(cargo_key: &str) -> String {
    let texts: Vec<&str> = trailer_keys_for_cargo(cargo_key)
        .iter()
        .filter_map(|key| trailer_type(key).map(|t| t.equipment_text))
        .collect();
    or_list(&texts, "carrier trailer")
}

/// The catalogue keys among `programs`, first occurrence only, in order.
pub fn normalized_trailer_programs<I, S>(programs: I) -> Vec<&'static str>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut seen: Vec<&'static str> = Vec::new();
    for key in programs {
        if let Some(trailer) = trailer_type(key.as_ref()) {
            if !seen.contains(&trailer.key) {
                seen.push(trailer.key);
            }
        }
    }
    seen
}

pub fn compatible_with_programs<I, S>(cargo_key: &str, programs: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let owned = normalized_trailer_programs(programs);
    trailer_keys_for_cargo(cargo_key)
        .iter()
        .any(|key| owned.contains(key))
}

pub fn required_program_text(cargo_key: &str) -> String {
    trailer_labels(trailer_keys_for_cargo(cargo_key))
}

pub fn trailer_program_charge_per_mile(cargo_key: &str) -> f64 {
    trailer_keys_for_cargo(cargo_key)
        .iter()
        .filter_map(|key| trailer_type(key).map(|t| t.per_mile_reserve))
        .fold(None, |best: Option<f64>, charge| {
            Some(best.map_or(charge, |b| b.max(charge)))
        })
        .unwrap_or_else(|| {
            trailer_type("dry_van")
                .expect("dry van is in the catalog")
                .per_mile_reserve
        })
}

pub fn owned_trailer_for_cargo<I, S>(
    cargo_key: &str,
    owned_trailers: I,
) -> Option<&'static TrailerType>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let owned = normalized_trailer_programs(owned_trailers);
    trailer_keys_for_cargo(cargo_key)
        .iter()
        .find(|key| owned.contains(key))
        .and_then(|key| trailer_type(key))
}

pub fn owned_trailer_charge_per_mile<I, S>(cargo_key: &str, owned_trailers: I) -> Option<f64>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    owned_trailer_for_cargo(cargo_key, owned_trailers).map(|t| t.owned_per_mile_reserve)
}

#[cfg(test)]
mod tests {
    //! Ported from the trailer cases of `tests/test_business_arc.py`,
    //! `tests/test_tanker_surge.py` and `tests/test_trailer_yard.py`.

    use super::*;

    #[test]
    fn test_trailer_catalog_matches_current_cargo_classes() {
        for key in ["dry_van", "reefer", "flatbed", "bulk"] {
            assert!(is_trailer_key(key));
        }
        assert_eq!(trailer_keys_for_cargo("general"), ["dry_van"]);
        assert_eq!(trailer_keys_for_cargo("refrigerated"), ["reefer"]);
        assert_eq!(trailer_keys_for_cargo("steel"), ["flatbed"]);
        assert_eq!(trailer_keys_for_cargo("grain"), ["bulk"]);
        assert!(compatible_with_programs("farm_inputs", ["dry_van"]));
        assert!(compatible_with_programs("farm_inputs", ["bulk"]));
        let reefer = trailer_type("reefer").unwrap();
        assert!(reefer.purchase_price > reefer.lease_deposit);
        assert!(reefer.owned_per_mile_reserve < reefer.per_mile_reserve);
    }

    #[test]
    fn test_tank_freight_is_gated_to_the_back_half_of_the_career() {
        // The trailer half of the tanker-surge case: both liquid classes
        // need the tank, and only the tank. The CARGO_CATALOG half (min
        // level, endorsement) belongs to models::jobs.
        for key in ["fuel_bulk", "liquid_food"] {
            assert_eq!(trailer_keys_for_cargo(key), ["tank"]);
        }
    }

    #[test]
    fn unknown_cargo_falls_back_on_the_default_program() {
        assert_eq!(
            trailer_keys_for_cargo("hovercraft"),
            DEFAULT_TRAILER_PROGRAMS
        );
        assert_eq!(required_program_text("hovercraft"), "Dry van");
        assert_eq!(equipment_text_for_cargo("hovercraft"), "dry van trailer");
    }

    #[test]
    fn labels_and_equipment_text_join_with_or() {
        assert_eq!(required_program_text("container"), "Dry van, or Flatbed");
        assert_eq!(required_program_text("construction"), "Flatbed, or Dry van");
        assert_eq!(
            equipment_text_for_cargo("farm_inputs"),
            "dry van trailer, or bulk or hopper trailer"
        );
        assert_eq!(
            trailer_labels(["dry_van", "reefer", "tank"]),
            "Dry van, Reefer, or Tank"
        );
        assert_eq!(trailer_labels(["nope"]), "carrier trailer");
        assert_eq!(trailer_labels(Vec::<String>::new()), "carrier trailer");
    }

    #[test]
    fn normalized_programs_drop_unknown_and_duplicate_keys() {
        assert_eq!(
            normalized_trailer_programs(["reefer", "bogus", "dry_van", "reefer"]),
            ["reefer", "dry_van"]
        );
        assert!(!compatible_with_programs("steel", ["dry_van", "reefer"]));
        assert!(compatible_with_programs("lumber_paper", ["dry_van"]));
    }

    #[test]
    fn program_charge_is_the_dearest_compatible_program() {
        assert_eq!(trailer_program_charge_per_mile("general"), 0.12);
        assert_eq!(trailer_program_charge_per_mile("farm_inputs"), 0.20);
        assert_eq!(trailer_program_charge_per_mile("fuel_bulk"), 0.26);
        assert_eq!(trailer_program_charge_per_mile("nothing"), 0.12);
    }

    #[test]
    fn owned_trailer_lookup_prefers_the_cargo_order() {
        let owned = ["dry_van", "flatbed"];
        assert_eq!(
            owned_trailer_for_cargo("container", owned).unwrap().key,
            "dry_van"
        );
        assert_eq!(
            owned_trailer_for_cargo("construction", owned).unwrap().key,
            "flatbed"
        );
        assert_eq!(
            owned_trailer_charge_per_mile("construction", owned),
            Some(0.07)
        );
        assert_eq!(owned_trailer_charge_per_mile("food", owned), None);
        assert!(owned_trailer_for_cargo("food", Vec::<String>::new()).is_none());
    }

    #[test]
    fn the_catalog_keys_are_unique_and_stable() {
        let keys: Vec<&str> = TRAILER_CATALOG.iter().map(|t| t.key).collect();
        assert_eq!(
            keys,
            ["dry_van", "reefer", "flatbed", "bulk", "tank", "double_van"]
        );
        assert_eq!(TANK_CAPACITY_TONS, 26.0);
    }

    #[test]
    fn doubles_freight_needs_the_twin_trailer_program() {
        for key in ["parcel_doubles", "turnpike_doubles"] {
            assert_eq!(trailer_keys_for_cargo(key), ["double_van"]);
        }
        assert_eq!(
            equipment_text_for_cargo("parcel_doubles"),
            "twin 28-foot trailers"
        );
    }

    #[test]
    fn each_doubles_load_names_its_own_trailers() {
        use crate::models::jobs::cargo_type;
        let parcel = cargo_type("parcel_doubles").unwrap();
        assert_eq!(parcel.equipment_text(), "twin 28-foot trailers");
        // Pups of parcel run out of room long before an 80,000-pound gross.
        assert!(parcel.weight_tons.1 <= 15.0);
        let turnpike = cargo_type("turnpike_doubles").unwrap();
        assert_eq!(turnpike.equipment_text(), "two 48-foot trailers");
    }
}
