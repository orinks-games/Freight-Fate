//! What a placarded load actually is: a real material from the 49 CFR
//! 172.101 Hazardous Materials Table, with its hazard class, so a tunnel or
//! bridge turns away only the classes it bans.

use crate::data::world_models::HazardClass;

/// One placarded material the catalog's hazardous freight can be.
#[derive(Debug, Clone, PartialEq)]
pub struct HazmatCommodity {
    /// Save key.
    pub key: &'static str,
    /// Spoken name.
    pub spoken: &'static str,
    /// The 172.101 proper shipping name and identification number.
    pub shipping_name: &'static str,
    pub un_number: &'static str,
    pub class: HazardClass,
    /// The catalog cargo it rides as.
    pub cargo: &'static str,
}

const fn packaged(
    key: &'static str,
    spoken: &'static str,
    shipping_name: &'static str,
    un_number: &'static str,
    class: HazardClass,
) -> HazmatCommodity {
    HazmatCommodity {
        key,
        spoken,
        shipping_name,
        un_number,
        class,
        cargo: "hazardous",
    }
}

/// Drums, cylinders and bags on a dry van (`hazardous`), and the bulk fuel
/// tanker's gasoline (`fuel_bulk`). Classes read from the 49 CFR 172.101
/// table (eCFR, accessed 2026-10-11).
pub const HAZMAT_COMMODITIES: &[HazmatCommodity] = &[
    packaged(
        "paint",
        "paint",
        "Paint",
        "UN1263",
        HazardClass::FlammableLiquid,
    ),
    packaged(
        "propane_cylinders",
        "propane cylinders",
        "Petroleum gases, liquefied",
        "UN1075",
        HazardClass::FlammableGas,
    ),
    packaged(
        "nitrogen_cylinders",
        "compressed nitrogen",
        "Nitrogen, compressed",
        "UN1066",
        HazardClass::NonflammableGas,
    ),
    packaged(
        "chlorine_cylinders",
        "chlorine cylinders",
        "Chlorine",
        "UN1017",
        HazardClass::PoisonGas,
    ),
    packaged(
        "calcium_carbide",
        "calcium carbide",
        "Calcium carbide",
        "UN1402",
        HazardClass::DangerousWhenWet,
    ),
    packaged(
        "pool_chlorine",
        "pool chlorine",
        "Calcium hypochlorite, dry",
        "UN1748",
        HazardClass::Oxidizer,
    ),
    packaged(
        "sulfuric_acid",
        "sulfuric acid",
        "Sulfuric acid with more than 51 percent acid",
        "UN1830",
        HazardClass::Corrosive,
    ),
    packaged(
        "consumer_fireworks",
        "consumer fireworks",
        "Fireworks",
        "UN0336",
        HazardClass::Explosive14,
    ),
    packaged(
        "blasting_explosives",
        "blasting explosives",
        "Explosive, blasting, type E",
        "UN0241",
        HazardClass::Explosive11,
    ),
    HazmatCommodity {
        key: "gasoline",
        spoken: "gasoline",
        shipping_name: "Gasoline",
        un_number: "UN1203",
        class: HazardClass::FlammableLiquid,
        cargo: "fuel_bulk",
    },
];

impl HazmatCommodity {
    /// What the load card names: shipping name, UN number and class, e.g.
    /// `Gasoline, UN1203, Class 3`.
    pub fn card_text(&self) -> String {
        format!(
            "{}, {}, Class {}",
            self.shipping_name,
            self.un_number,
            self.class.number()
        )
    }
}

pub fn hazmat_commodity(key: &str) -> Option<&'static HazmatCommodity> {
    HAZMAT_COMMODITIES.iter().find(|c| c.key == key)
}

/// A saved material named by key, UN number (`UN1203` or `1203`), shipping
/// name or spoken name, so a save that names its load any of those ways
/// keeps its class.
pub fn hazmat_commodity_named(text: &str) -> Option<&'static HazmatCommodity> {
    let text = text.trim();
    let digits = text
        .strip_prefix("UN")
        .or_else(|| text.strip_prefix("un"))
        .unwrap_or(text)
        .trim();
    hazmat_commodity(text).or_else(|| {
        HAZMAT_COMMODITIES.iter().find(|c| {
            c.un_number[2..] == *digits
                || c.shipping_name.eq_ignore_ascii_case(text)
                || c.spoken.eq_ignore_ascii_case(text)
        })
    })
}

/// The material a load of this cargo carries. Bulk fuel is always
/// gasoline; packaged hazardous freight is picked from the lane, by a
/// stable hash rather than the board's random stream, so the rest of the
/// board is drawn exactly as before.
pub fn commodity_for(cargo_key: &str, lane: &str) -> Option<&'static HazmatCommodity> {
    let choices: Vec<&'static HazmatCommodity> = HAZMAT_COMMODITIES
        .iter()
        .filter(|c| c.cargo == cargo_key)
        .collect();
    if choices.len() <= 1 {
        return choices.first().copied();
    }
    let hash = lane.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    choices.get((hash % choices.len() as u64) as usize).copied()
}

/// What `Job::new` gives a load before the board names its material:
/// gasoline for bulk fuel, nothing for anything else.
pub fn default_commodity(cargo_key: &str) -> Option<&'static HazmatCommodity> {
    (cargo_key == "fuel_bulk")
        .then(|| hazmat_commodity("gasoline"))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_material_rides_a_placarded_catalog_cargo() {
        for c in HAZMAT_COMMODITIES {
            assert!(matches!(c.cargo, "hazardous" | "fuel_bulk"), "{}", c.key);
            assert!(c.un_number.starts_with("UN") && c.un_number.len() == 6);
            assert_eq!(hazmat_commodity(c.key), Some(c));
        }
        assert_eq!(
            commodity_for("fuel_bulk", "a|b").map(|c| c.key),
            Some("gasoline")
        );
        assert_eq!(commodity_for("general", "a|b"), None);
    }

    #[test]
    fn the_load_card_names_material_un_number_and_class() {
        assert_eq!(
            hazmat_commodity("gasoline").unwrap().card_text(),
            "Gasoline, UN1203, Class 3"
        );
        assert_eq!(
            hazmat_commodity("chlorine_cylinders").unwrap().card_text(),
            "Chlorine, UN1017, Class 2.3"
        );
        assert_eq!(
            hazmat_commodity_named("un1203").map(|c| c.key),
            Some("gasoline")
        );
        assert_eq!(hazmat_commodity_named("rocks"), None);
    }

    #[test]
    fn packaged_freight_varies_by_lane_and_is_stable() {
        let pick = |lane: &str| commodity_for("hazardous", lane).map(|c| c.key);
        assert_eq!(pick("x|y"), pick("x|y"));
        let distinct: std::collections::HashSet<_> =
            (0..200).filter_map(|i| pick(&format!("lane{i}"))).collect();
        assert_eq!(distinct.len(), 9);
    }

    #[test]
    fn the_material_saves_and_an_old_save_without_one_still_loads() {
        use crate::models::jobs::{cargo_type, job_from_payload, job_payload, Job};
        let cargo = cargo_type("hazardous").unwrap();
        let mut job = Job::new(cargo, 10.0, "a", "Dock", "b", 100.0, 900.0, 10.0);
        assert_eq!(job.hazmat, None);
        job.hazmat = hazmat_commodity("chlorine_cylinders");
        let mut saved = job_payload(&job);
        assert_eq!(job_from_payload(&saved).unwrap().hazmat, job.hazmat);
        saved.remove("hazmat");
        let old = job_from_payload(&saved).expect("an old save loads");
        assert_eq!(old.hazmat, None);
        assert_eq!(old.crossing_load().hazard_class, None);
        assert!(
            !old.crossing_load().placarded,
            "a load of unknown class is carried unplacarded, not as explosives"
        );
        saved.insert("hazmat".into(), "UN1017".into());
        assert_eq!(
            job_from_payload(&saved).unwrap().hazmat.map(|m| m.key),
            Some("chlorine_cylinders"),
            "a save naming the UN number keeps the class"
        );
        saved.insert("hazmat".into(), "Paint".into());
        assert_eq!(
            job_from_payload(&saved).unwrap().hazmat.map(|m| m.key),
            Some("paint"),
            "a save naming the material keeps the class"
        );
        saved.remove("hazmat");
        saved.insert("un_number".into(), "1066".into());
        assert_eq!(
            job_from_payload(&saved).unwrap().hazmat.map(|m| m.key),
            Some("nitrogen_cylinders")
        );
        saved.remove("un_number");
        saved.insert("hazmat".into(), "gasoline".into());
        assert_eq!(
            job_from_payload(&saved).unwrap().hazmat,
            None,
            "a material is kept only on the cargo it rides as"
        );
        let fuel = Job::new(
            cargo_type("fuel_bulk").unwrap(),
            20.0,
            "a",
            "Dock",
            "b",
            1.0,
            1.0,
            1.0,
        );
        let mut saved = job_payload(&fuel);
        saved.remove("hazmat");
        assert_eq!(
            job_from_payload(&saved).unwrap().hazmat.map(|m| m.class),
            Some(HazardClass::FlammableLiquid)
        );
    }
}
