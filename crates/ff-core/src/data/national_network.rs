//! National Network access for STAA twin 28-foot doubles (`parcel_doubles`).
//!
//! Real US twin pups have federally guaranteed access only on the National
//! Network (23 CFR 658: the Interstate System plus designated federal-aid
//! primary routes) and on reasonable access off that network to terminals
//! and to food, fuel, repair, and rest (23 CFR 658.19 federal floor is one
//! mile; state rules may grant more). World data does not yet carry NN
//! designation flags, so this module uses an honest conservative
//! approximation:
//!
//! * **National Network** ≈ Interstate designation (`I-n` via
//!   [`crate::data::grades::road_class`]). Designated US and state primary
//!   routes land later as explicit per-segment flags checked against the
//!   FHWA National Network map, not by road class.
//! * **Reasonable access** ≈ same-city local legs (`a == b`, uncapped — the
//!   facility-approach form), plus first/last end legs with `local_cue` or
//!   plain stubs, both capped at [`REASONABLE_ACCESS_MAX_MI`]. Mid-route
//!   connectors never count.
//! * **Corridor policy:** twin loads are refused on ALCAN, Canada, and
//!   Alaska lanes (any origin, destination, or leg outside the lower-48 US)
//!   until provincial and Alaska doubles rules are modeled.
//!
//! Honesty debts: Interstate-only under-includes real NN; access distance
//! varies by state beyond the 1.0 mi floor; non-lower-48 doubles are not
//! offered yet.

use crate::data::grades::road_class;
use crate::data::world_models::{City, Leg, Route};

/// Federal-floor reasonable-access distance (23 CFR 658.19), in miles.
/// States may grant more; the game does not model those extensions yet.
pub const REASONABLE_ACCESS_MAX_MI: f64 = 1.0;

/// Spoken when no twin-legal National Network route remains.
pub const STAA_DOUBLES_ROUTE_REFUSAL: &str = "Dispatch can't clear twin trailers on that run. It \
     leaves the National Network.";

/// Spoken when dispatch drops one or more lanes that leave the NN.
pub const STAA_DOUBLES_REROUTE_NOTE: &str =
    "Dispatch dropped a lane that leaves the National Network.";

/// Spoken when the lane is outside the lower-48 US (ALCAN, Canada, Alaska).
pub const STAA_DOUBLES_CORRIDOR_REFUSAL: &str = "Dispatch doesn't run twins on that lane.";

/// Whether a leg's highway designation is treated as National Network under
/// the Interstate-only approximation.
pub fn leg_on_national_network(leg: &Leg) -> bool {
    road_class(&leg.highway) == "interstate"
}

/// Same-city local approach (position-free). End-of-route `local_cue` and
/// short stubs are decided only in [`route_allows_staa_doubles`].
pub fn leg_is_reasonable_access(leg: &Leg) -> bool {
    leg.a == leg.b
}

/// Whether a city record sits outside the lower-48 United States.
pub fn city_outside_lower_48(city: &City) -> bool {
    city.country != "US" || city.state_code == "AK" || city.state_code == "HI"
}

/// Parse `{name}_{state}_{country}` city keys (e.g. `tok_ak_us`,
/// `surrey_bc_ca`). Returns `(state, country)` lowercased when the trailing
/// two segments look like a region code pair.
pub fn city_key_state_country(city_key: &str) -> Option<(&str, &str)> {
    let (rest, country) = city_key.rsplit_once('_')?;
    let (_name, state) = rest.rsplit_once('_')?;
    if state.len() == 2 && country.len() == 2 {
        Some((state, country))
    } else {
        None
    }
}

/// Whether a city key names a place outside the lower-48 US. Unparseable
/// synthetic keys (unit-test stubs like `"A"`) are treated as inside so NN
/// tests stay focused.
pub fn city_key_outside_lower_48(city_key: &str) -> bool {
    match city_key_state_country(city_key) {
        Some((state, country)) => {
            let country = country.to_ascii_uppercase();
            let state = state.to_ascii_uppercase();
            country != "US" || state == "AK" || state == "HI"
        }
        None => false,
    }
}

/// Whether any city on the route (path cities or leg endpoints) sits outside
/// the lower-48 US.
pub fn route_outside_lower_48(route: &Route) -> bool {
    route
        .cities
        .iter()
        .any(|city| city_key_outside_lower_48(city))
        || route
            .legs
            .iter()
            .any(|leg| city_key_outside_lower_48(&leg.a) || city_key_outside_lower_48(&leg.b))
}

fn leg_is_end_access(leg: &Leg) -> bool {
    // Same-city locals (facility approaches from `Leg::local`) are uncapped:
    // they are the approach itself, not a highway stub.
    if leg_is_reasonable_access(leg) {
        return true;
    }
    // First/last cue'd approach or plain stub: federal-floor mileage cap.
    if !leg.local_cue.is_empty() {
        return leg.miles <= REASONABLE_ACCESS_MAX_MI;
    }
    leg.miles <= REASONABLE_ACCESS_MAX_MI
}

/// Whether every leg of a route is STAA-legal under the game's approximation:
/// Interstate (NN), same-city local, or a first/last facility approach / short
/// terminal stub. Mid-route connectors never count as access.
pub fn route_allows_staa_doubles(route: &Route) -> bool {
    let n = route.legs.len();
    if n == 0 {
        return false;
    }
    route.legs.iter().enumerate().all(|(i, leg)| {
        if leg_on_national_network(leg) {
            return true;
        }
        // Same-city local may sit anywhere on a facility chain.
        if leg_is_reasonable_access(leg) {
            return true;
        }
        let at_end = i == 0 || i + 1 == n;
        at_end && leg_is_end_access(leg)
    })
}

/// Keep only lower-48 routes STAA doubles may run. Empty means refuse.
pub fn filter_staa_doubles_routes(routes: &[Route]) -> Vec<Route> {
    routes
        .iter()
        .filter(|route| !route_outside_lower_48(route) && route_allows_staa_doubles(route))
        .cloned()
        .collect()
}

/// Whether a cargo class must stay on the National Network approximation.
pub fn cargo_requires_national_network(cargo_key: &str) -> bool {
    cargo_key == "parcel_doubles"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::world_models::Leg;

    fn interstate_leg() -> Leg {
        Leg::new("A", "B", 40.0, "I-80", "flat", Vec::new())
    }

    fn us_leg() -> Leg {
        Leg::new("A", "B", 40.0, "US-50", "flat", Vec::new())
    }

    fn state_leg() -> Leg {
        Leg::new("A", "B", 40.0, "CA-99", "flat", Vec::new())
    }

    fn approach_leg() -> Leg {
        Leg::local(
            "denver_co_us",
            0.8,
            "Dock Road",
            "right onto Dock Road",
            25.0,
        )
    }

    fn short_connector() -> Leg {
        Leg::new("yard", "A", 1.0, "Frontage Rd", "flat", Vec::new())
    }

    fn mid_route_local_cue_leg() -> Leg {
        let mut leg = Leg::new("B", "C", 2.0, "Dock Spur", "flat", Vec::new());
        leg.local_cue = "right onto Dock Spur".to_string();
        leg
    }

    #[test]
    fn interstate_leg_is_national_network() {
        assert!(leg_on_national_network(&interstate_leg()));
    }

    #[test]
    fn us_and_state_legs_are_not_national_network_yet() {
        // Honesty: designated US/state NN segments come later as explicit
        // per-segment flags against the FHWA National Network map.
        assert!(!leg_on_national_network(&us_leg()));
        assert!(!leg_on_national_network(&state_leg()));
    }

    #[test]
    fn facility_approach_leg_is_reasonable_access() {
        let approach = approach_leg();
        assert!(leg_is_reasonable_access(&approach));
        assert!(leg_is_end_access(&approach));
    }

    #[test]
    fn doubles_job_on_nn_route_is_allowed() {
        let route = Route::from_legs(
            vec!["A".into(), "B".into(), "C".into()],
            vec![
                Leg::new("A", "B", 50.0, "I-70", "flat", Vec::new()),
                Leg::new("B", "C", 60.0, "I-25", "hills", Vec::new()),
            ],
        );
        assert!(route_allows_staa_doubles(&route));
    }

    #[test]
    fn non_nn_leg_is_refused() {
        let route = Route::from_legs(vec!["A".into(), "B".into()], vec![us_leg()]);
        assert!(!route_allows_staa_doubles(&route));
    }

    #[test]
    fn reasonable_access_approach_on_nn_route_is_allowed() {
        let route = Route::from_legs(
            vec!["dock".into(), "A".into(), "B".into()],
            vec![
                approach_leg(),
                Leg::new("A", "B", 80.0, "I-80", "flat", Vec::new()),
            ],
        );
        assert!(route_allows_staa_doubles(&route));
    }

    #[test]
    fn short_terminal_stub_at_either_end_is_allowed() {
        let out = Route::from_legs(
            vec!["yard".into(), "A".into(), "B".into()],
            vec![
                short_connector(),
                Leg::new("A", "B", 90.0, "I-5", "flat", Vec::new()),
            ],
        );
        assert!(route_allows_staa_doubles(&out));

        let into = Route::from_legs(
            vec!["A".into(), "B".into(), "yard".into()],
            vec![
                Leg::new("A", "B", 90.0, "I-5", "flat", Vec::new()),
                short_connector(),
            ],
        );
        assert!(route_allows_staa_doubles(&into));
    }

    #[test]
    fn end_stub_over_one_mile_is_refused() {
        let stub = Leg::new("yard", "A", 1.01, "Frontage Rd", "flat", Vec::new());
        let route = Route::from_legs(
            vec!["yard".into(), "A".into(), "B".into()],
            vec![stub, Leg::new("A", "B", 90.0, "I-5", "flat", Vec::new())],
        );
        assert!(!route_allows_staa_doubles(&route));
    }

    #[test]
    fn cue_d_end_leg_over_one_mile_is_refused() {
        // A first/last leg with a local_cue still has to fit the 1.0 mi access
        // cap. Same-city facility approaches (a == b) stay uncapped.
        let mut long_cue = Leg::new("yard", "A", 2.5, "Dock Spur", "flat", Vec::new());
        long_cue.local_cue = "right onto Dock Spur".to_string();
        let route = Route::from_legs(
            vec!["yard".into(), "A".into(), "B".into()],
            vec![
                long_cue,
                Leg::new("A", "B", 90.0, "I-5", "flat", Vec::new()),
            ],
        );
        assert!(!route_allows_staa_doubles(&route));

        let same_city = Leg::local(
            "denver_co_us",
            2.5,
            "Dock Road",
            "right onto Dock Road",
            25.0,
        );
        let ok = Route::from_legs(
            vec!["dock".into(), "A".into(), "B".into()],
            vec![
                same_city,
                Leg::new("A", "B", 90.0, "I-5", "flat", Vec::new()),
            ],
        );
        assert!(route_allows_staa_doubles(&ok));
    }

    #[test]
    fn long_non_nn_mid_leg_is_refused_even_beside_interstate() {
        let route = Route::from_legs(
            vec!["A".into(), "B".into(), "C".into()],
            vec![
                Leg::new("A", "B", 40.0, "I-80", "flat", Vec::new()),
                Leg::new("B", "C", 40.0, "US-50", "flat", Vec::new()),
            ],
        );
        assert!(!route_allows_staa_doubles(&route));
    }

    #[test]
    fn mid_route_local_cue_leg_is_refused() {
        let route = Route::from_legs(
            vec!["A".into(), "B".into(), "C".into(), "D".into()],
            vec![
                Leg::new("A", "B", 40.0, "I-80", "flat", Vec::new()),
                mid_route_local_cue_leg(),
                Leg::new("C", "D", 40.0, "I-80", "flat", Vec::new()),
            ],
        );
        assert!(!route_allows_staa_doubles(&route));
    }

    #[test]
    fn mid_route_short_connector_never_counts_as_access() {
        let route = Route::from_legs(
            vec!["A".into(), "B".into(), "C".into(), "D".into()],
            vec![
                Leg::new("A", "B", 40.0, "I-80", "flat", Vec::new()),
                Leg::new("B", "C", 0.5, "Frontage Rd", "flat", Vec::new()),
                Leg::new("C", "D", 40.0, "I-80", "flat", Vec::new()),
            ],
        );
        assert!(!route_allows_staa_doubles(&route));
    }

    #[test]
    fn filter_drops_non_nn_options_and_keeps_nn() {
        let nn = Route::from_legs(vec!["A".into(), "B".into()], vec![interstate_leg()]);
        let off = Route::from_legs(vec!["A".into(), "B".into()], vec![us_leg()]);
        let kept = filter_staa_doubles_routes(&[off, nn.clone()]);
        assert_eq!(kept.len(), 1);
        assert!(route_allows_staa_doubles(&kept[0]));
    }

    #[test]
    fn only_parcel_doubles_requires_the_network() {
        assert!(cargo_requires_national_network("parcel_doubles"));
        assert!(!cargo_requires_national_network("general"));
        assert!(!cargo_requires_national_network("turnpike_doubles"));
        assert!(!cargo_requires_national_network("parcel"));
    }

    #[test]
    fn city_keys_detect_alaska_canada_and_lower_48() {
        assert!(city_key_outside_lower_48("tok_ak_us"));
        assert!(city_key_outside_lower_48("fairbanks_ak_us"));
        assert!(city_key_outside_lower_48("anchorage_ak_us"));
        assert!(city_key_outside_lower_48("surrey_bc_ca"));
        assert!(city_key_outside_lower_48("whitehorse_yt_ca"));
        assert!(!city_key_outside_lower_48("seattle_wa_us"));
        assert!(!city_key_outside_lower_48("chicago_il_us"));
        assert!(!city_key_outside_lower_48("salt_lake_city_ut_us"));
        assert!(!city_key_outside_lower_48("A")); // synthetic
    }

    #[test]
    fn alcan_canada_alaska_routes_are_outside_lower_48() {
        let ak = Route::from_legs(
            vec!["tok_ak_us".into(), "fairbanks_ak_us".into()],
            vec![Leg::new(
                "tok_ak_us",
                "fairbanks_ak_us",
                200.0,
                "AK-2",
                "flat",
                Vec::new(),
            )],
        );
        assert!(route_outside_lower_48(&ak));

        let canada = Route::from_legs(
            vec!["surrey_bc_ca".into(), "whitehorse_yt_ca".into()],
            vec![Leg::new(
                "surrey_bc_ca",
                "whitehorse_yt_ca",
                1000.0,
                "Hwy 1",
                "flat",
                Vec::new(),
            )],
        );
        assert!(route_outside_lower_48(&canada));

        let through = Route::from_legs(
            vec![
                "seattle_wa_us".into(),
                "surrey_bc_ca".into(),
                "tok_ak_us".into(),
            ],
            vec![
                Leg::new(
                    "seattle_wa_us",
                    "surrey_bc_ca",
                    120.0,
                    "I-5",
                    "flat",
                    Vec::new(),
                ),
                Leg::new(
                    "surrey_bc_ca",
                    "tok_ak_us",
                    1800.0,
                    "Hwy 1",
                    "flat",
                    Vec::new(),
                ),
            ],
        );
        assert!(route_outside_lower_48(&through));
    }

    #[test]
    fn filter_refuses_outside_lower_48_even_on_interstate() {
        // An Interstate-labelled Alaska stub still fails corridor policy.
        let route = Route::from_legs(
            vec!["anchorage_ak_us".into(), "fairbanks_ak_us".into()],
            vec![Leg::new(
                "anchorage_ak_us",
                "fairbanks_ak_us",
                350.0,
                "I-999",
                "flat",
                Vec::new(),
            )],
        );
        assert!(route_allows_staa_doubles(&route)); // NN shape alone
        assert!(filter_staa_doubles_routes(&[route]).is_empty());
    }

    #[test]
    fn corridor_refusal_line_is_distinct_from_national_network_line() {
        assert_eq!(
            STAA_DOUBLES_CORRIDOR_REFUSAL,
            "Dispatch doesn't run twins on that lane."
        );
        assert_ne!(STAA_DOUBLES_CORRIDOR_REFUSAL, STAA_DOUBLES_ROUTE_REFUSAL);
        assert!(!STAA_DOUBLES_CORRIDOR_REFUSAL.contains("National Network"));
    }
}
