//! Where a roadside billboard may stand: each state's or province's sign law,
//! as one table the trip reads at placement time.
//!
//! Three answers cover North America. Most states and provinces license
//! commercial boards on private land beside the highway. A few license only
//! boards for the things a traveler stops for (a museum, a lodge, a hot
//! spring), so the anywhere pool and Big Jim stay off their roads while a
//! placed attraction sign still stands. The rest ban commercial boards
//! outright, and the road is silent there. Parks and scenic routes inside an
//! open jurisdiction are not here: they are `billboard_bans` spans on the
//! legs that cross them.
//!
//! `docs/billboard-spider-methodology.md` carries the same table with its
//! reasoning; a test keeps the two in step.

/// What a jurisdiction's sign law lets stand beside its highways.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardLaw {
    /// Commercial boards are licensed: every kind of billboard may stand.
    Open,
    /// Only boards for visitor attractions and traveler services: placed
    /// attraction signs stand, the anywhere pool does not.
    AttractionsOnly,
    /// No commercial boards at all.
    Banned,
}

impl BoardLaw {
    /// Whether an everyday pool sign (the anywhere, corridor and regional
    /// lines, and Big Jim's swaps) may be read here.
    pub fn allows_pool(self) -> bool {
        self == BoardLaw::Open
    }

    /// Whether a placed attraction sign (`billboard_sign`) may be read here.
    pub fn allows_placed(self) -> bool {
        self != BoardLaw::Banned
    }

    /// The word the methodology table uses for this answer.
    pub fn table_word(self) -> &'static str {
        match self {
            BoardLaw::Open => "open",
            BoardLaw::AttractionsOnly => "attractions only",
            BoardLaw::Banned => "none",
        }
    }
}

/// One state, province or territory whose sign law the table records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Jurisdiction {
    /// The two-letter postal code the world's cities carry.
    pub code: &'static str,
    /// The name the state-miles bake uses.
    pub name: &'static str,
    pub law: BoardLaw,
    /// The statute or official page the answer was read from.
    pub source: &'static str,
}

const fn row(
    code: &'static str,
    name: &'static str,
    law: BoardLaw,
    source: &'static str,
) -> Jurisdiction {
    Jurisdiction {
        code,
        name,
        law,
        source,
    }
}

use BoardLaw::{AttractionsOnly, Banned, Open};

/// Every jurisdiction with a recorded answer. A state not listed here
/// licenses commercial boards (the United States pass applied each state's
/// scenic-byway rules as leg spans instead).
pub const BOARD_LAWS: &[Jurisdiction] = &[
    // United States: the four states that opted out of commercial billboards
    // (Scenic America; FHWA outdoor advertising control).
    row("ME", "Maine", Banned, "Scenic America; FHWA outdoor advertising control"),
    row("VT", "Vermont", Banned, "Scenic America; FHWA outdoor advertising control"),
    row("AK", "Alaska", Banned, "Scenic America; FHWA outdoor advertising control"),
    row("HI", "Hawaii", Banned, "Scenic America; FHWA outdoor advertising control"),
    // Canada, read 2026-11-22. Each row: the official page or statute.
    row(
        "BC",
        "British Columbia",
        Open,
        "BC Ministry of Transportation, Permits and Approvals Manual s 1.6.2: billboards stand on private land at least 300 m from a provincial highway, never on the right of way",
    ),
    row(
        "AB",
        "Alberta",
        Banned,
        "Alberta Highways Development and Protection Act ss 11 to 19; alberta.ca: off-premise advertising billboards are not permitted along highways",
    ),
    row(
        "SK",
        "Saskatchewan",
        Open,
        "Saskatchewan Provincial Highway Sign Control Regulations; Roadside Management Manual 1220-00: commercial signs on private property outside the right of way, by permit within 400 m",
    ),
    row(
        "MB",
        "Manitoba",
        Open,
        "Manitoba Transportation Infrastructure Act; gov.mb.ca: private advertising signs allowed outside the right of way with a permit",
    ),
    row(
        "ON",
        "Ontario",
        Open,
        "Ontario Public Transportation and Highway Improvement Act; MTO Corridor Signing Policy: billboards by permit within 400 m of a provincial highway",
    ),
    row(
        "QC",
        "Quebec",
        Open,
        "Quebec Roadside Advertising Act, CQLR c P-44 ss 7, 13, 17: commercial signs within 300 m of a highway by permit, none at designated scenic sites",
    ),
    row(
        "NB",
        "New Brunswick",
        AttractionsOnly,
        "New Brunswick Highway Act, Highway Advertisements Regulation 97-143 s 5: on Level I and II highways, the trucking routes, only tourism businesses may sign",
    ),
    row(
        "NS",
        "Nova Scotia",
        AttractionsOnly,
        "Nova Scotia Public Highways Act s 49, Advertising Signs Regulations ss 4, 5: licences only for lodging, restaurants, attractions and similar traveler businesses",
    ),
    row(
        "PE",
        "Prince Edward Island",
        Banned,
        "Prince Edward Island Highway Signage Act; Regulations s 3.1: no off-premises sign within view of a highway",
    ),
    row(
        "NL",
        "Newfoundland and Labrador",
        Banned,
        "Newfoundland and Labrador Highway Sign Regulations, NLR 85/99; Off-Site Promotional Highway Signage Policy: only government tourism panels, no billboard permits",
    ),
    row(
        "YT",
        "Yukon",
        AttractionsOnly,
        "Yukon Highways Act, Visitor Information Sign Regulation: billboards only for businesses offering visitor services, facilities or attractions",
    ),
    row(
        "NT",
        "Northwest Territories",
        Banned,
        "Northwest Territories Public Highways Act, Highway Signs Regulations s 6(2): no permit for a sign advertising a commercial business",
    ),
    row(
        "NU",
        "Nunavut",
        Banned,
        "Nunavut Public Highways Act, Highway Signs Regulations s 6: no permit for a sign advertising a commercial business",
    ),
];

/// The recorded jurisdiction for a postal code or a bake name, if any.
pub fn jurisdiction(code_or_name: &str) -> Option<&'static Jurisdiction> {
    let key = code_or_name.trim();
    BOARD_LAWS
        .iter()
        .find(|j| j.code.eq_ignore_ascii_case(key) || j.name.eq_ignore_ascii_case(key))
}

/// What may stand in a jurisdiction. One not in the table licenses boards.
pub fn board_law(code_or_name: &str) -> BoardLaw {
    jurisdiction(code_or_name).map_or(Open, |j| j.law)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CANADA: [&str; 13] = [
        "BC", "AB", "SK", "MB", "ON", "QC", "NB", "NS", "PE", "NL", "YT", "NT", "NU",
    ];

    #[test]
    fn every_province_and_territory_has_an_answer() {
        for code in CANADA {
            let row = jurisdiction(code).unwrap_or_else(|| panic!("{code} missing"));
            assert!(!row.source.trim().is_empty(), "{code} has no source");
        }
    }

    #[test]
    fn codes_and_names_both_resolve() {
        assert_eq!(board_law("Yukon"), AttractionsOnly);
        assert_eq!(board_law("YT"), AttractionsOnly);
        assert_eq!(board_law("british columbia"), Open);
        assert_eq!(board_law("Alaska"), Banned);
        assert_eq!(board_law("Texas"), Open);
        assert_eq!(board_law(""), Open);
    }

    #[test]
    fn what_each_answer_lets_stand() {
        assert!(Open.allows_pool() && Open.allows_placed());
        assert!(!AttractionsOnly.allows_pool() && AttractionsOnly.allows_placed());
        assert!(!Banned.allows_pool() && !Banned.allows_placed());
    }

    /// The methodology doc's table and this one say the same thing.
    #[test]
    fn the_methodology_table_matches() {
        let doc = include_str!("../../../../docs/billboard-spider-methodology.md");
        for row in BOARD_LAWS.iter().filter(|r| CANADA.contains(&r.code)) {
            let line = doc
                .lines()
                .find(|l| l.starts_with(&format!("| {} |", row.name)))
                .unwrap_or_else(|| panic!("no table row for {}", row.name));
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            assert_eq!(
                cells.get(2).copied(),
                Some(row.law.table_word()),
                "{} differs from the doc",
                row.name
            );
        }
    }
}
