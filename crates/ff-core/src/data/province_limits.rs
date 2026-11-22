//! The speed limit a Canadian road takes where the map has no posting: each
//! province's and territory's statutory default, the "unless otherwise
//! posted" number in its traffic act.
//!
//! The United States fallback is a road-class figure in miles per hour (60
//! on a state route), which in Canada reads as a converted number nobody
//! signs ("Speed limit raised to 97"). A Canadian mile with no posting takes
//! the row here instead, so it speaks a round number in kilometers per hour.
//!
//! Every value is ASSUMED for the road: the map does not know the stretch is
//! unsigned, only that OpenStreetMap carries no `maxspeed` there, and the
//! statute's default is the published figure that fills the gap. Each row
//! also says how the statute was read: from the act itself, or from a
//! government handbook or a secondary page that cites it, where the act
//! could not be fetched on 2026-11-22.

/// One province or territory's default limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProvinceLimit {
    /// The two-letter postal code the world's cities carry.
    pub code: &'static str,
    /// The name the state-miles bake uses.
    pub name: &'static str,
    /// The default on a rural highway outside a built-up area, in km/h.
    pub rural_kmh: u32,
    /// The default on a freeway or autoroute, where the statute sets one.
    pub freeway_kmh: Option<u32>,
    /// Where the number comes from.
    pub source: &'static str,
    /// How the statute was read: the act itself, or a page that cites it.
    pub statute_basis: StatuteBasis,
}

/// How a row's statute was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatuteBasis {
    /// The section was read in the act's official text.
    Read,
    /// The act could not be fetched; the figure comes from a government
    /// handbook or a secondary page that states it.
    Secondary,
    /// The statute sets no number; the row is a fallback.
    Assumed,
}

impl ProvinceLimit {
    /// The default for a road of this highway label, in km/h.
    pub fn kmh_for(&self, highway: &str) -> u32 {
        match self.freeway_kmh {
            Some(kmh) if is_freeway_label(highway) => kmh,
            _ => self.rural_kmh,
        }
    }

    /// [`Self::kmh_for`] in the miles per hour the sim runs on.
    pub fn mph_for(&self, highway: &str) -> f64 {
        f64::from(self.kmh_for(highway)) / crate::units::MILES_TO_KM
    }
}

/// Whether every Canadian road in a leg's label is a freeway by its number:
/// Quebec's autoroutes ("A-20", "A-15 / I-87"). A label that also names a
/// numbered route ("A-15 / Route 117") is not, since part of the leg runs on
/// the ordinary road.
fn is_freeway_label(highway: &str) -> bool {
    let canadian: Vec<&str> = highway
        .split('/')
        .map(str::trim)
        .filter(|part| !part.starts_with("I-") && !part.starts_with("US "))
        .collect();
    !canadian.is_empty() && canadian.iter().all(|part| part.starts_with("A-"))
}

const fn row(
    code: &'static str,
    name: &'static str,
    rural_kmh: u32,
    freeway_kmh: Option<u32>,
    statute_basis: StatuteBasis,
    source: &'static str,
) -> ProvinceLimit {
    ProvinceLimit {
        code,
        name,
        rural_kmh,
        freeway_kmh,
        source,
        statute_basis,
    }
}

use StatuteBasis::{Assumed, Read, Secondary};

/// Every province and territory, read 2026-11-22.
pub const PROVINCE_LIMITS: &[ProvinceLimit] = &[
    row(
        "BC",
        "British Columbia",
        80,
        None,
        Read,
        "Motor Vehicle Act, RSBC 1996 c 318, s 146(1): 80 km/h on a highway outside a municipality",
    ),
    row(
        "AB",
        "Alberta",
        100,
        None,
        Secondary,
        "Traffic Safety Act, RSA 2000 c T-6, as summarized by Wheatland County (2021): unless otherwise posted, 100 km/h on two-lane undivided highways, 80 km/h on other rural roads",
    ),
    row(
        "SK",
        "Saskatchewan",
        80,
        None,
        Secondary,
        "Traffic Safety Act, SS 2004 c T-18.1, Part XVI Division 2; SGI Driver's Handbook: in the absence of signs, the speed limit is 80 km/h",
    ),
    row(
        "MB",
        "Manitoba",
        90,
        None,
        Read,
        "The Highway Traffic Act, CCSM c H60, s 94.2: 90 km/h outside a restricted speed area",
    ),
    row(
        "ON",
        "Ontario",
        80,
        Some(80),
        Read,
        "Highway Traffic Act, RSO 1990 c H.8, s 128(1)(b.1) and (c): 80 km/h on a highway not within a local municipality or built-up area, and on a controlled-access highway",
    ),
    row(
        "QC",
        "Quebec",
        90,
        Some(100),
        Secondary,
        "Highway Safety Code, CQLR c C-24.2, s 328, as stated by quebec.ca and Wikipedia's Speed limits in Canada: 100 km/h on autoroutes, 90 km/h on paved roads",
    ),
    row(
        "NB",
        "New Brunswick",
        80,
        None,
        Secondary,
        "Motor Vehicle Act, RSNB 1973 c M-17; New Brunswick Driver's Handbook, Part 4, as cited by Wikipedia's Speed limits in Canada: 80 km/h",
    ),
    row(
        "NS",
        "Nova Scotia",
        80,
        None,
        Secondary,
        "Motor Vehicle Act, RSNS 1989 c 293; Nova Scotia Driver's Handbook, Chapter 2: unless otherwise posted, the maximum speed on any highway is 80 km/h",
    ),
    row(
        "PE",
        "Prince Edward Island",
        90,
        None,
        Secondary,
        "Highway Traffic Act, RSPEI 1988 c H-5, as cited by Wikipedia's Speed limits in Canada: 100 km/h by day and 90 km/h by night on paved highways; the night figure is used",
    ),
    row(
        "NL",
        "Newfoundland and Labrador",
        80,
        None,
        Secondary,
        "Highway Traffic Act, RSNL 1990 c H-3, as cited by Wikipedia's Speed limits in Canada: 80 km/h on paved roads outside the Trans-Canada Highway, which takes 100",
    ),
    row(
        "YT",
        "Yukon",
        80,
        None,
        Assumed,
        "Motor Vehicles Act, RSY 2002 c 153, as cited by Wikipedia's Speed limits in Canada: the rural limit is left to the road authority, with no number; 80 km/h assumed, the lowest provincial rural default",
    ),
    row(
        "NT",
        "Northwest Territories",
        90,
        None,
        Secondary,
        "Motor Vehicles Act, RSNWT 1988 c M-16; NWT Basic Driver's Manual: 90 km/h on a primary highway outside a settlement unless otherwise posted",
    ),
    row(
        "NU",
        "Nunavut",
        90,
        None,
        Secondary,
        "Traffic Safety Act, as cited by Wikipedia's Speed limits in Canada: 90 km/h",
    ),
];

/// The row for a province or territory by the name the state-miles bake
/// uses, or `None` outside Canada.
pub fn province_limit(name: &str) -> Option<&'static ProvinceLimit> {
    PROVINCE_LIMITS.iter().find(|row| row.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_province_and_territory_has_a_row() {
        let codes: Vec<&str> = PROVINCE_LIMITS.iter().map(|r| r.code).collect();
        for code in [
            "BC", "AB", "SK", "MB", "ON", "QC", "NB", "NS", "PE", "NL", "YT", "NT", "NU",
        ] {
            assert!(codes.contains(&code), "{code} has no default limit row");
        }
        assert_eq!(codes.len(), 13);
    }

    #[test]
    fn every_default_is_a_round_number_in_kilometers() {
        for r in PROVINCE_LIMITS {
            assert_eq!(r.rural_kmh % 10, 0, "{}", r.code);
            if let Some(kmh) = r.freeway_kmh {
                assert_eq!(kmh % 10, 0, "{}", r.code);
            }
            assert!(!r.source.is_empty(), "{}", r.code);
        }
    }

    #[test]
    fn autoroutes_take_the_freeway_figure_and_mixed_labels_do_not() {
        let qc = province_limit("Quebec").unwrap();
        assert_eq!(qc.kmh_for("A-20"), 100);
        assert_eq!(qc.kmh_for("A-55 / I-91"), 100);
        assert_eq!(qc.kmh_for("A-15 / Route 117"), 90);
        assert_eq!(qc.kmh_for("Route 117"), 90);
        assert_eq!(province_limit("Ontario").unwrap().kmh_for("Hwy 17"), 80);
        assert!(province_limit("Minnesota").is_none());
    }
}
