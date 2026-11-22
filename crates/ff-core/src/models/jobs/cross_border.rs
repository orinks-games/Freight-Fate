//! Loads across the Canadian border, as a US carrier may book them.
//!
//! The driver works for a US carrier, so the real rule applies: a US
//! carrier may haul a load from the United States into Canada and a load
//! from Canada back out, and may not haul between two Canadian points
//! (cabotage; Canada Border Services Agency Memorandum D3-1-5,
//! "International commercial transportation",
//! https://www.cbsa-asfc.gc.ca/publications/dm-md/d3/d3-1-5-eng.html).
//! Canadian carriers, Canadian
//! home bases and loads inside Canada wait for a later slice.
//!
//! What may cross is narrower than what may run at home: twin parcel
//! trailers stay on the US National Network, live cattle need export
//! health papers and inspections the game does not model, and placarded
//! loads need Canada's Transportation of Dangerous Goods paperwork, which
//! the game does not model either. Those three stay off Canadian lanes.

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

/// Whether a US carrier may book a load from a city in `origin_country` to
/// one in `destination_country` (two-letter codes, any case): inside the
/// United States, into Canada, or out of Canada to the United States.
/// Never between two Canadian points, and nowhere else.
pub fn lane_allowed(origin_country: &str, destination_country: &str) -> bool {
    let origin = origin_country.to_ascii_uppercase();
    let destination = destination_country.to_ascii_uppercase();
    matches!(
        (origin.as_str(), destination.as_str()),
        ("US", "US") | ("US", "CA") | ("CA", "US")
    )
}

/// Whether a lane between two countries crosses the border.
pub fn lane_crosses(origin_country: &str, destination_country: &str) -> bool {
    !origin_country.eq_ignore_ascii_case(destination_country)
}

/// Whether this carrier books loads across the border. A carrier in
/// `carriers.json` says so itself (`cross_border`); a start with no carrier
/// record is a driver on their own authority, who may.
pub fn carrier_runs_to_canada(carrier_key: &str) -> bool {
    carrier(carrier_key).is_none_or(|c| c.cross_border)
}

/// Whether this freight may be booked across the border at all.
pub fn cargo_may_cross(national_network: bool, livestock: bool, placarded: bool) -> bool {
    !(national_network || livestock || placarded)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lanes_follow_cabotage() {
        let table = [
            ("US", "US", true),
            ("US", "CA", true),
            ("CA", "US", true),
            ("CA", "CA", false),
            ("us", "ca", true),
            ("US", "MX", false),
            ("MX", "US", false),
        ];
        for (origin, destination, allowed) in table {
            assert_eq!(
                lane_allowed(origin, destination),
                allowed,
                "{origin} to {destination}"
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
    }
}
