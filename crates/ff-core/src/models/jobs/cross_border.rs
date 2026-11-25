//! The lane law: which loads a carrier may book across, through and
//! inside the two countries on the map, and what crosses at all.
//!
//! Every board, relay and playtest offer asks one table, [`lane_kind`] and
//! [`lane_permitted`], with the carrier's base country and authority.
//!
//! - A carrier may haul a load from its own country into the other one and
//!   back out with the authority and bonded paperwork for it
//!   (`cross_border` on the carrier).
//! - A carrier may never haul between two points in the other country
//!   (cabotage). Canada Border Services Agency Memorandum D3-1-5,
//!   "International commercial transportation",
//!   https://www.cbsa-asfc.gc.ca/publications/dm-md/d3/d3-1-5-eng.html ;
//!   for the driver's side, J. J. Keller, "Canadian cabotage rules for
//!   U.S.-based drivers operating in Canada",
//!   https://jjkellercompliancenetwork.com/regsense/canadian-cabotage-rules-for-us-based-drivers-operating-in-canada
//!   and "U.S. cabotage rules for Canadian-based drivers operating in the
//!   United States",
//!   https://jjkellercompliancenetwork.com/regsense/us-cabotage-rules-for-canadian-based-drivers-operating-in-the-united-states .
//! - A load between two points in the carrier's own country whose road runs
//!   through the other one (the lower 48 and Alaska through Canada, Anchorage
//!   and Skagway through the Yukon) travels in bond and needs the same
//!   authority. A load between two points in the other country whose road
//!   runs through the carrier's own country is still cabotage: never.
//! - ASSUMED: a Canadian carrier may run a Canadian load through the United
//!   States in bond, the mirror of the US rule. No page read says either way.
//!
//! What may cross is narrower than what may run at home: twin parcel
//! trailers stay on the US National Network, live cattle need export
//! health papers and inspections the game does not model, and placarded
//! loads need Canada's Transportation of Dangerous Goods paperwork, which
//! the game does not model either. Those three stay off every lane that
//! leaves the country, through traffic included. A load between Alaska and
//! the lower 48 by road carries road freight only ([`ROAD_FREIGHT`]): the
//! rest of Alaska's freight goes by sea.

use crate::data::national_network::city_key_state_country;
use crate::data::world_models::Route;
use crate::models::carriers::carrier;

/// Minutes at the primary booth. ASSUMPTION: neither agency publishes a
/// per-port commercial processing time (CBP's wait-times feed has no Alcan
/// entry); the same number the booth itself uses.
pub const CUSTOMS_PRIMARY_MIN: f64 = 10.0;
/// Extra minutes when a truck is referred to secondary inspection.
/// ASSUMPTION: no published per-port secondary processing time exists.
pub const CUSTOMS_SECONDARY_EXTRA_MIN: f64 = 45.0;
/// How often a truck is referred to secondary. ASSUMPTION: no published
/// commercial referral rate exists.
pub const CUSTOMS_SECONDARY_REFERRAL_CHANCE: f64 = 0.1;

/// Hours a deadline books for one crossing. Derived: the primary booth plus
/// a secondary inspection, so a referral never by itself makes a load late
/// (a dispatcher books the worse case at a port, not the average).
pub const CUSTOMS_PLANNED_H: f64 = (CUSTOMS_PRIMARY_MIN + CUSTOMS_SECONDARY_EXTRA_MIN) / 60.0;

/// The two-letter country in a city key (`surrey_bc_ca` is "ca"); None for
/// a key without one, which counts as the United States.
fn country_of_key(key: &str) -> &str {
    city_key_state_country(key).map(|(_, c)| c).unwrap_or("us")
}

/// The country a carrier is based in. A start with no carrier record is a
/// driver on their own US authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CarrierBase {
    #[default]
    Us,
    Ca,
}

impl CarrierBase {
    /// From a two-letter country code, any case; None for any other country.
    pub fn from_code(code: &str) -> Option<Self> {
        match code.to_ascii_uppercase().as_str() {
            "US" => Some(Self::Us),
            "CA" => Some(Self::Ca),
            _ => None,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Us => "US",
            Self::Ca => "CA",
        }
    }
}

/// What a lane is, relative to the carrier's base country.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneKind {
    /// Both ends in the base country, and the road stays there.
    Home,
    /// The ends are in different countries.
    Border,
    /// Both ends in the base country, and the road enters the other one.
    HomeTransit,
    /// Both ends in the other country, and the road stays there.
    Cabotage,
    /// Both ends in the other country, and the road enters the base country.
    ForeignTransit,
}

impl LaneKind {
    /// Whether the load leaves the country it was loaded in on the way.
    pub fn leaves_country(self) -> bool {
        matches!(
            self,
            Self::Border | Self::HomeTransit | Self::ForeignTransit
        )
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Border => "border",
            Self::HomeTransit => "home_transit",
            Self::Cabotage => "cabotage",
            Self::ForeignTransit => "foreign_transit",
        }
    }
}

/// The lane between two countries (two-letter codes, any case) for a
/// carrier based in `base`, where `route_enters_other_country` says whether
/// the supported road leaves the end country on the way. None when either
/// end is in a country other than the United States and Canada.
pub fn lane_kind(
    base: CarrierBase,
    origin_country: &str,
    destination_country: &str,
    route_enters_other_country: bool,
) -> Option<LaneKind> {
    let origin = CarrierBase::from_code(origin_country)?;
    let destination = CarrierBase::from_code(destination_country)?;
    Some(match (origin == destination, origin == base) {
        (false, _) => LaneKind::Border,
        (true, true) if route_enters_other_country => LaneKind::HomeTransit,
        (true, true) => LaneKind::Home,
        (true, false) if route_enters_other_country => LaneKind::ForeignTransit,
        (true, false) => LaneKind::Cabotage,
    })
}

/// Whether a carrier may haul a load on this lane: home always, across or
/// through the other country only with `authority`, cabotage never, and
/// nothing outside the two countries.
pub fn lane_permitted(kind: Option<LaneKind>, authority: bool) -> bool {
    match kind {
        Some(LaneKind::Home) => true,
        Some(LaneKind::Border | LaneKind::HomeTransit) => authority,
        Some(LaneKind::Cabotage | LaneKind::ForeignTransit) | None => false,
    }
}

/// The base country of a carrier key; `Us` for a start with no record.
///
/// A leased owner-operator start has no carrier record of its own: its base
/// is its lessor's, found the way the hiring carrier is, by name.
pub fn carrier_base(carrier_key: &str) -> CarrierBase {
    #[cfg(test)]
    if let Some((base, _)) = fixture::carrier(carrier_key) {
        return base;
    }
    carrier(carrier_key)
        .or_else(|| {
            crate::models::start_options::all_start_options()
                .into_iter()
                .find(|option| option.key == carrier_key)
                .and_then(crate::models::start_options::hiring_carrier_for_option)
        })
        .map_or(CarrierBase::Us, |c| c.base)
}

/// Whether this carrier books loads across the border. A carrier in
/// `carriers.json` says so itself (`cross_border`); a start with no carrier
/// record is a driver on their own authority, who may.
pub fn carrier_runs_to_canada(carrier_key: &str) -> bool {
    #[cfg(test)]
    if let Some((_, authority)) = fixture::carrier(carrier_key) {
        return authority;
    }
    carrier(carrier_key).is_none_or(|c| c.cross_border)
}

/// The lane for this carrier between two countries.
pub fn carrier_lane(
    carrier_key: &str,
    origin_country: &str,
    destination_country: &str,
    route_enters_other_country: bool,
) -> Option<LaneKind> {
    lane_kind(
        carrier_base(carrier_key),
        origin_country,
        destination_country,
        route_enters_other_country,
    )
}

/// Whether this carrier may haul between two countries on this road.
pub fn carrier_may_haul(
    carrier_key: &str,
    origin_country: &str,
    destination_country: &str,
    route_enters_other_country: bool,
) -> bool {
    lane_permitted(
        carrier_lane(
            carrier_key,
            origin_country,
            destination_country,
            route_enters_other_country,
        ),
        carrier_runs_to_canada(carrier_key),
    )
}

/// Freight that goes between Alaska and the lower 48 by road, as named
/// data with the source of each. Everything else sails from Tacoma.
pub const ROAD_FREIGHT: &[(&str, &str)] = &[
    (
        "steel",
        "READ: Mercer Transportation, \"full truckload shipments, heavy equipment, oversized \
         cargo, and anything that requires flatbed or specialized trailer transport\", \
         https://mercer-trans.com/2026/02/09/shipping-freight-to-alaska",
    ),
    ("lumber_paper", "READ: Mercer Transportation, as for steel"),
    ("machinery", "READ: Mercer Transportation, as for steel"),
    ("construction", "READ: Mercer Transportation, as for steel"),
    (
        "food",
        "ASSUMED: seafood and other food going south by truck; no page read gives a figure",
    ),
    ("refrigerated", "ASSUMED: as for food"),
];

/// Whether a cargo is road freight on a lane through the other country.
pub fn road_freight(cargo_key: &str) -> bool {
    ROAD_FREIGHT.iter().any(|(key, _)| *key == cargo_key)
}

/// Whether this cargo, with or without a placard, may ride this lane:
/// anything at home; freight that may cross on a border lane; and on a
/// lane through the other country, freight that may cross and is road
/// freight as well.
pub fn cargo_fits_lane(kind: LaneKind, cargo: &super::catalog::CargoType, placarded: bool) -> bool {
    let may_cross = cargo_may_cross(
        cargo.national_network,
        cargo.key == crate::sim::livestock::LIVESTOCK_CARGO_KEY,
        placarded,
    );
    match kind {
        LaneKind::Home | LaneKind::Cabotage => true,
        LaneKind::Border => may_cross,
        LaneKind::HomeTransit | LaneKind::ForeignTransit => may_cross && road_freight(cargo.key),
    }
}

/// Whether this freight may be booked across the border at all.
pub fn cargo_may_cross(national_network: bool, livestock: bool, placarded: bool) -> bool {
    !(national_network || livestock || placarded)
}

/// Whether a catalog cargo is ever placarded: any cargo the hazmat table
/// names a material for, whatever the lane.
pub fn cargo_ever_placarded(cargo_key: &str) -> bool {
    super::hazmat::HAZMAT_COMMODITIES
        .iter()
        .any(|commodity| commodity.cargo == cargo_key)
}

/// Whether a catalog cargo may ride this lane before its material is
/// drawn: [`cargo_fits_lane`] with a placard for any cargo that can carry
/// one.
pub fn cargo_type_fits_lane(kind: LaneKind, cargo: &super::catalog::CargoType) -> bool {
    cargo_fits_lane(kind, cargo, cargo_ever_placarded(cargo.key))
}

/// Whether a catalog cargo may be booked across the border: the same rule
/// as [`cargo_may_cross`], read from the cargo itself. Placarded freight is
/// any cargo the hazmat table names a material for, whatever the lane.
pub fn cargo_type_may_cross(cargo: &super::catalog::CargoType) -> bool {
    let placarded = cargo_ever_placarded(cargo.key);
    cargo_may_cross(
        cargo.national_network,
        cargo.key == crate::sim::livestock::LIVESTOCK_CARGO_KEY,
        placarded,
    )
}

/// Border crossings on a route: each leg whose two cities are in different
/// countries is one customs stop, at the inbound country's booth.
pub fn route_crossings(route: &Route) -> usize {
    route
        .cities
        .windows(2)
        .filter(|pair| !country_of_key(&pair[0]).eq_ignore_ascii_case(country_of_key(&pair[1])))
        .count()
}

/// Hours a deadline books at the border for this route.
pub fn route_customs_hours(route: Option<&Route>) -> f64 {
    route.map_or(0.0, |r| route_crossings(r) as f64 * CUSTOMS_PLANNED_H)
}

/// A carrier for one test thread only, for the lanes of a carrier
/// `carriers.json` does not have yet (a Canadian base). The board caches hold
/// nothing that depends on the carrier, so a fixture never leaks into
/// another test.
#[cfg(test)]
pub(crate) mod fixture {
    use super::CarrierBase;
    use std::cell::RefCell;
    use std::collections::HashMap;

    thread_local! {
        static CARRIERS: RefCell<HashMap<String, (CarrierBase, bool)>> =
            RefCell::new(HashMap::new());
    }

    /// Run `f` with `key` standing for a carrier of this base and authority.
    pub(crate) fn with_carrier<R>(
        key: &str,
        base: CarrierBase,
        authority: bool,
        f: impl FnOnce() -> R,
    ) -> R {
        CARRIERS.with(|c| c.borrow_mut().insert(key.to_string(), (base, authority)));
        let out = f();
        CARRIERS.with(|c| c.borrow_mut().remove(key));
        out
    }

    pub(super) fn carrier(key: &str) -> Option<(CarrierBase, bool)> {
        CARRIERS.with(|c| c.borrow().get(key).copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every base, both end countries, through or not, and authority.
    #[test]
    fn the_lane_table() {
        use CarrierBase::{Ca, Us};
        use LaneKind::*;
        // (base, origin, destination, road enters the other country, kind)
        let kinds = [
            (Us, "US", "US", false, Some(Home)),
            (Us, "US", "US", true, Some(HomeTransit)),
            (Us, "US", "CA", false, Some(Border)),
            (Us, "CA", "US", true, Some(Border)),
            (Us, "CA", "CA", false, Some(Cabotage)),
            (Us, "CA", "CA", true, Some(ForeignTransit)),
            (Ca, "CA", "CA", false, Some(Home)),
            (Ca, "CA", "CA", true, Some(HomeTransit)),
            (Ca, "CA", "US", false, Some(Border)),
            (Ca, "US", "CA", false, Some(Border)),
            (Ca, "US", "US", false, Some(Cabotage)),
            (Ca, "US", "US", true, Some(ForeignTransit)),
            (Us, "us", "ca", false, Some(Border)),
            (Us, "US", "MX", false, None),
            (Us, "MX", "US", false, None),
            (Ca, "MX", "MX", false, None),
        ];
        for (base, origin, destination, through, kind) in kinds {
            assert_eq!(
                lane_kind(base, origin, destination, through),
                kind,
                "{base:?} {origin} to {destination}, through {through}"
            );
        }
        // (kind, authority, may haul)
        let law = [
            (Some(Home), false, true),
            (Some(Home), true, true),
            (Some(Border), false, false),
            (Some(Border), true, true),
            (Some(HomeTransit), false, false),
            (Some(HomeTransit), true, true),
            (Some(Cabotage), false, false),
            (Some(Cabotage), true, false),
            (Some(ForeignTransit), false, false),
            (Some(ForeignTransit), true, false),
            (None, false, false),
            (None, true, false),
        ];
        for (kind, authority, allowed) in law {
            assert_eq!(
                lane_permitted(kind, authority),
                allowed,
                "{kind:?} with authority {authority}"
            );
        }
    }

    #[test]
    fn road_freight_is_the_flatbed_and_cold_list() {
        let cargo = |key: &str| super::super::catalog::cargo_type(key).expect("a catalog cargo");
        assert!(cargo_type_fits_lane(LaneKind::HomeTransit, cargo("steel")));
        assert!(cargo_type_fits_lane(
            LaneKind::HomeTransit,
            cargo("refrigerated")
        ));
        assert!(!cargo_type_fits_lane(
            LaneKind::HomeTransit,
            cargo("general")
        ));
        assert!(!cargo_type_fits_lane(
            LaneKind::HomeTransit,
            cargo("retail")
        ));
        assert!(cargo_type_fits_lane(LaneKind::Border, cargo("general")));
        assert!(cargo_type_fits_lane(LaneKind::Home, cargo("hazardous")));
        assert!(!cargo_type_fits_lane(LaneKind::Border, cargo("hazardous")));
        for (key, source) in ROAD_FREIGHT {
            assert!(
                super::super::catalog::cargo_type(key).is_some(),
                "{key} is not a cargo"
            );
            assert!(
                source.starts_with("READ") || source.starts_with("ASSUMED"),
                "{key}: {source}"
            );
        }
    }

    #[test]
    fn a_route_counts_each_leg_that_changes_country() {
        let route = Route {
            cities: vec![
                "seattle_wa_us".into(),
                "bellingham_wa_us".into(),
                "blaine_wa_us".into(),
                "surrey_bc_ca".into(),
                "prince_george_bc_ca".into(),
            ],
            ..Route::default()
        };
        assert_eq!(route_crossings(&route), 1);
        assert!((route_customs_hours(Some(&route)) - 55.0 / 60.0).abs() < 1e-9);
        // Alaska through the Yukon crosses twice.
        let through = Route {
            cities: vec![
                "surrey_bc_ca".into(),
                "blaine_wa_us".into(),
                "tok_ak_us".into(),
                "whitehorse_yt_ca".into(),
            ],
            ..Route::default()
        };
        assert_eq!(route_crossings(&through), 2);
        assert_eq!(route_customs_hours(None), 0.0);
    }

    #[test]
    fn twins_cattle_and_placards_stay_home() {
        assert!(cargo_may_cross(false, false, false));
        assert!(!cargo_may_cross(true, false, false));
        assert!(!cargo_may_cross(false, true, false));
        assert!(!cargo_may_cross(false, false, true));
    }

    #[test]
    fn the_carriers_that_run_to_canada_are_data() {
        assert!(carrier_runs_to_canada("northstar"));
        assert!(carrier_runs_to_canada("chehalis_freight"));
        assert!(!carrier_runs_to_canada("lone_mesa_freight"));
        assert!(!carrier_runs_to_canada("great_lakes_training"));
        assert!(carrier_runs_to_canada("roadstead_owner_operator"));
        assert_eq!(carrier_base("northstar"), CarrierBase::Us);
        assert_eq!(carrier_base("roadstead_owner_operator"), CarrierBase::Us);
    }
}
