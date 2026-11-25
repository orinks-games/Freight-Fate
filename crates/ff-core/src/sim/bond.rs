//! The papers a load travels under at the border: one state, moved only by
//! [`BondState::next`].
//!
//! A load that crosses once (a border lane) files its manifest and closes it
//! at its one booth. A load between two points in one country whose road
//! runs through the other (the lower 48 and Alaska through Canada) is
//! cleared into the other country in bond, rides through it in bond, and
//! the bond closes at the booth back into the country where it ends. An
//! empty run, or a road that never leaves the country, has no papers.

use crate::data::world_models::Route;
use crate::models::jobs::cross_border::route_crossings;
use crate::sim::trip_models::BorderBooth;

/// Where a load's border papers stand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BondState {
    /// The road never leaves the country, or the truck runs empty.
    #[default]
    None,
    /// The load will cross; the manifest is filed.
    Filed,
    /// A home load is inside the other country.
    InBond,
    /// Cleared into the country where the load ends.
    Closed,
}

/// What moves the papers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BondEvent {
    /// A booth cleared the truck into a country: the load's destination
    /// country, or the other one on the way.
    Cleared { into_destination_country: bool },
}

/// What the booth checks, and so what the officer says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoothPapers {
    /// The eManifest of a load crossing once, or no load at all: the lines
    /// every border lane has.
    Ordinary,
    /// A home load entering the other country in transit: the in-transit
    /// manifest and the bond.
    EnteringInBond,
    /// A home load coming back into its own country: the bond closes.
    ClosingBond,
}

impl BondState {
    /// The papers a trip starts with on this road: filed when a load's road
    /// changes country at all, none for an empty run or a road at home.
    pub fn for_route(route: &Route, loaded: bool) -> Self {
        if loaded && route_crossings(route) > 0 {
            Self::Filed
        } else {
            Self::None
        }
    }

    /// The one place the papers move.
    pub fn next(self, event: BondEvent) -> Self {
        match (self, event) {
            (Self::None, _) => Self::None,
            (Self::Closed, _) => Self::Closed,
            (
                Self::Filed | Self::InBond,
                BondEvent::Cleared {
                    into_destination_country: true,
                },
            ) => Self::Closed,
            (
                Self::Filed | Self::InBond,
                BondEvent::Cleared {
                    into_destination_country: false,
                },
            ) => Self::InBond,
        }
    }

    /// What a booth into the destination country (or not) checks in this
    /// state.
    pub fn papers(self, into_destination_country: bool) -> BoothPapers {
        match (self, into_destination_country) {
            (Self::Filed, false) => BoothPapers::EnteringInBond,
            (Self::InBond, true) => BoothPapers::ClosingBond,
            _ => BoothPapers::Ordinary,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Filed => "filed",
            Self::InBond => "in_bond",
            Self::Closed => "closed",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Some(match key {
            "none" => Self::None,
            "filed" => Self::Filed,
            "in_bond" => Self::InBond,
            "closed" => Self::Closed,
            _ => return None,
        })
    }
}

impl crate::sim::trip::Trip {
    /// The two-letter country, upper case, the route ends in.
    pub fn destination_country(&self) -> String {
        self.route
            .cities
            .last()
            .and_then(|key| self.world.cities.get(key))
            .map(|city| city.country.to_ascii_uppercase())
            .unwrap_or_default()
    }

    /// Whether the rest of this trip stays in Canada: the route ends in
    /// Canada and no border booth still ahead enters another country. A
    /// Canadian load whose road runs through the United States is not, until
    /// the truck is back over the line.
    pub fn road_ahead_stays_in_canada(&self) -> bool {
        self.destination_country() == "CA"
            && !self.border_booths.iter().any(|booth| {
                booth.at_mi > self.position_mi && !booth.entering_country.eq_ignore_ascii_case("CA")
            })
    }

    fn booth_enters_destination_country(&self, booth: &BorderBooth) -> bool {
        booth
            .entering_country
            .eq_ignore_ascii_case(&self.destination_country())
    }

    /// What this booth checks on this load.
    pub fn booth_papers(&self, booth: &BorderBooth) -> BoothPapers {
        self.bond
            .papers(self.booth_enters_destination_country(booth))
    }

    /// The booth cleared the truck: the papers move.
    pub fn clear_booth(&mut self, booth: &BorderBooth) {
        let into_destination_country = self.booth_enters_destination_country(booth);
        self.bond = self.bond.next(BondEvent::Cleared {
            into_destination_country,
        });
    }

    /// The papers of a load on this road, as they stand once every booth
    /// before `position_mi` has cleared it: a drive staged or resumed
    /// partway, or a save from before the papers were kept.
    pub fn bond_at(&self, position_mi: f64) -> BondState {
        let mut bond = BondState::for_route(&self.route, !self.bobtail);
        for booth in self.border_booths.iter().filter(|b| b.at_mi < position_mi) {
            bond = bond.next(BondEvent::Cleared {
                into_destination_country: self.booth_enters_destination_country(booth),
            });
        }
        bond
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transitions() {
        use BondState::*;
        let into = |dest: bool| BondEvent::Cleared {
            into_destination_country: dest,
        };
        // (from, cleared into the destination country, to, what the booth checked)
        let table = [
            (None, false, None, BoothPapers::Ordinary),
            (None, true, None, BoothPapers::Ordinary),
            (Filed, true, Closed, BoothPapers::Ordinary),
            (Filed, false, InBond, BoothPapers::EnteringInBond),
            (InBond, true, Closed, BoothPapers::ClosingBond),
            (InBond, false, InBond, BoothPapers::Ordinary),
            (Closed, true, Closed, BoothPapers::Ordinary),
            (Closed, false, Closed, BoothPapers::Ordinary),
        ];
        for (from, dest, to, papers) in table {
            assert_eq!(
                from.papers(dest),
                papers,
                "{from:?} into destination {dest}"
            );
            assert_eq!(
                from.next(into(dest)),
                to,
                "{from:?} into destination {dest}"
            );
        }
    }

    #[test]
    fn keys_round_trip() {
        for state in [
            BondState::None,
            BondState::Filed,
            BondState::InBond,
            BondState::Closed,
        ] {
            assert_eq!(BondState::from_key(state.key()), Some(state));
        }
        assert_eq!(BondState::from_key("bonded"), None);
    }

    #[test]
    fn a_route_files_papers_only_when_loaded_and_crossing() {
        let route = |cities: &[&str]| Route {
            cities: cities.iter().map(|c| c.to_string()).collect(),
            ..Route::default()
        };
        let through = route(&["seattle_wa_us", "surrey_bc_ca", "tok_ak_us"]);
        assert_eq!(BondState::for_route(&through, true), BondState::Filed);
        assert_eq!(BondState::for_route(&through, false), BondState::None);
        let home = route(&["seattle_wa_us", "tacoma_wa_us"]);
        assert_eq!(BondState::for_route(&home, true), BondState::None);
    }
}
