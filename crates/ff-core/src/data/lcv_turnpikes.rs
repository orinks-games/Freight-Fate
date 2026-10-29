//! Permitted LCV turnpike corridors for `turnpike_doubles`.
//!
//! Real turnpike doubles (two long trailers) run only on specific toll roads
//! that authorize LCVs, and they make up / break down at staging lots at the
//! turnpike exits -- the combination itself stays on the permitted road.
//! World data does not tag "LCV permitted" on legs, so this module matches
//! an explicit allowlist of city-pair legs drawn from the shipped world graph
//! against the classic ISTEA-era turnpike systems (23 CFR 658 Appendix C),
//! plus short staging stubs at either end of a route.
//!
//! Honesty debts:
//! * Florida's Turnpike is mostly not Interstate-numbered in world data, so
//!   FL turnpike doubles are not offered under this approximation.
//! * Western LCV corridors (Rocky Mountain doubles and similar) are not
//!   modeled; only the classic turnpike corridor list counts.
//! * Staging lots are approximated as same-city locals / end `local_cue` /
//!   short end stubs, not curated break-bulk yards.
//! * No LCV driver certification or carrier permit gate (49 CFR 380).
//! * Hazmat in doubles is not modeled -- `turnpike_doubles` never carries
//!   placarded freight.
//! * World-data gap (pending map work): Ohio Turnpike east of Toledo to the
//!   PA line -- no I-80 city-pair that avoids Cleveland, and no Elyria node.
//! * World-data gap (pending map work): Kansas Turnpike I-70 Topeka to
//!   Kansas City -- no shipped leg (KC is in Missouri).
//! * NY I-87 Albany–Yonkers Thruway is not modeled as a city-pair: tandems
//!   cannot leave the Thruway onto public NY roads (TAP-602), the southern
//!   staging lot is Exit 6A Yonkers, and `new_york_ny_us` is refused as an
//!   origin or destination, so the coarse `new_york`–`albany` I-87 world leg
//!   is omitted rather than faked.

use crate::data::world_models::{Leg, Route};

/// Soft staging-lot stub at a turnpike exit (miles). Same federal-floor
/// spirit as STAA reasonable access; not a claim about every state's yard.
pub const LCV_STAGING_ACCESS_MAX_MI: f64 = 1.0;

/// Spoken when no permitted turnpike lane remains for `turnpike_doubles`.
pub const LCV_TURNPIKE_ROUTE_REFUSAL: &str = "Dispatch only clears long doubles on the turnpike.";

/// Spoken when dispatch drops a non-turnpike option for long doubles.
pub const LCV_TURNPIKE_REROUTE_NOTE: &str = "Dispatch dropped a lane that leaves the turnpike.";

/// Per-state LCV GVW caps on the classic turnpike systems, in pounds.
/// Enforced as the legal gross for a turnpike double through
/// `models::doubles::legal_gvw_lb_for_route`: the job board caps the load
/// and the live overweight check and scale ticket use the route's cap.
///
/// OH / IN / MA: 127,400 lb. NY: 143,000 lb. KS: 120,000 lb.
pub const LCV_TURNPIKE_GVW_CAP_LB: &[(&str, u32)] = &[
    ("OH", 127_400),
    ("IN", 127_400),
    ("MA", 127_400),
    ("NY", 143_000),
    ("KS", 120_000),
];

/// Allowed turnpike corridor legs as undirected city pairs plus highway,
/// taken from shipped `data/world_data/us/legs/*.json`.
///
/// Corridors covered:
/// * NY Thruway: I-90 Buffalo–Albany chain; Berkshire Section toward MA
///   (`albany`–`worcester` / `springfield`–`albany`). Buffalo–Erie (PA) is
///   refused -- the Thruway ends at the PA line and Pennsylvania allows no
///   turnpike doubles. I-87 Albany–NYC is omitted: NYC is a forbidden
///   endpoint (TAP-602 / Exit 6A Yonkers), and no Yonkers city node exists yet.
/// * Mass Pike: I-90 Boston–Worcester–Springfield (and the Albany link).
/// * Ohio Turnpike: I-80/I-90 from the IN line through Toledo
///   (`toledo`–`elkhart`). Cleveland and I-90 east of the Elyria split are
///   not listed (see exclusions in tests).
/// * Indiana Toll Road: I-80/I-90 Elkhart–South Bend–Gary.
/// * Kansas Turnpike: I-35 Wichita–Emporia, I-335 Emporia–Topeka. I-70
///   Topeka–KC is a documented world-data gap; I-70 west of Topeka is
///   intentionally omitted.
pub const LCV_TURNPIKE_LEGS: &[(&str, &str, &str)] = &[
    // NY Thruway I-90 Buffalo ↔ Albany
    ("buffalo_ny_us", "rochester_ny_us", "I-90"),
    ("rochester_ny_us", "syracuse_ny_us", "I-90"),
    ("syracuse_ny_us", "utica_ny_us", "I-90"),
    ("utica_ny_us", "albany_ny_us", "I-90"),
    ("syracuse_ny_us", "albany_ny_us", "I-90"),
    ("syracuse_ny_us", "buffalo_ny_us", "I-90"),
    // NY Thruway Berkshire Section ↔ MA line / Mass Pike link
    ("albany_ny_us", "worcester_ma_us", "I-90"),
    ("springfield_ma_us", "albany_ny_us", "I-90"),
    // Massachusetts Turnpike I-90
    ("boston_ma_us", "worcester_ma_us", "I-90"),
    ("springfield_ma_us", "worcester_ma_us", "I-90"),
    // Ohio Turnpike west (IN line ↔ Toledo); east-of-Elyria I-90 omitted
    ("toledo_oh_us", "elkhart_in_us", "I-80"),
    // Indiana Toll Road I-80 / I-90
    ("elkhart_in_us", "south_bend_in_us", "I-80"),
    ("gary_in_us", "south_bend_in_us", "I-90"),
    // Kansas Turnpike I-35 / I-335 (Wichita–Emporia–Topeka)
    ("emporia_ks_us", "wichita_ks_us", "I-35"),
    ("topeka_ks_us", "emporia_ks_us", "I-335"),
];

/// City keys that must never be a turnpike-doubles origin or destination.
/// Thruway tandems cannot leave the Thruway onto public NY roads (TAP-602);
/// the southernmost staging lot is Exit 6A in Yonkers, so neither end-stub
/// staging nor same-city locals can reach an NYC dock.
pub const LCV_TURNPIKE_FORBIDDEN_ENDPOINTS: &[&str] = &["new_york_ny_us"];

fn normalize_highway(highway: &str) -> String {
    highway.trim().to_uppercase().replace(' ', "")
}

fn cities_match(leg: &Leg, a: &str, b: &str) -> bool {
    (leg.a == a && leg.b == b) || (leg.a == b && leg.b == a)
}

/// Whether a city key may be a turnpike-doubles origin or destination.
pub fn city_allows_lcv_turnpike_endpoint(city_key: &str) -> bool {
    !LCV_TURNPIKE_FORBIDDEN_ENDPOINTS.contains(&city_key)
}

fn route_has_forbidden_endpoint(route: &Route) -> bool {
    if route
        .cities
        .first()
        .is_some_and(|c| !city_allows_lcv_turnpike_endpoint(c))
        || route
            .cities
            .last()
            .is_some_and(|c| !city_allows_lcv_turnpike_endpoint(c))
    {
        return true;
    }
    // End stubs / same-city locals: a forbidden city as either end of the
    // first or last leg means the combination would stage at that dock.
    if let Some(first) = route.legs.first() {
        if !city_allows_lcv_turnpike_endpoint(&first.a)
            || !city_allows_lcv_turnpike_endpoint(&first.b)
        {
            return true;
        }
    }
    if let Some(last) = route.legs.last() {
        if !city_allows_lcv_turnpike_endpoint(&last.a)
            || !city_allows_lcv_turnpike_endpoint(&last.b)
        {
            return true;
        }
    }
    false
}

/// GVW cap in pounds for a turnpike state, when one is recorded.
pub fn lcv_turnpike_gvw_cap_lb(state_code: &str) -> Option<u32> {
    let state = state_code.trim().to_uppercase();
    LCV_TURNPIKE_GVW_CAP_LB
        .iter()
        .find(|(st, _)| *st == state)
        .map(|(_, lb)| *lb)
}

/// Whether a corridor leg is on the explicit LCV turnpike allowlist.
pub fn leg_on_lcv_turnpike(leg: &Leg) -> bool {
    let hwy = normalize_highway(&leg.highway);
    if hwy.is_empty() {
        return false;
    }
    LCV_TURNPIKE_LEGS
        .iter()
        .any(|(a, b, listed)| normalize_highway(listed) == hwy && cities_match(leg, a, b))
}

fn leg_is_staging_access(leg: &Leg) -> bool {
    if leg.a == leg.b {
        return true;
    }
    // A plain stub and a first/last cue'd approach share one cap, the same
    // 1.0 mi the STAA National Network gate applies to cue'd end legs.
    leg.miles <= LCV_STAGING_ACCESS_MAX_MI
}

/// Whether every leg is on a permitted turnpike, or a first/last staging
/// access stub / same-city local.
pub fn route_allows_lcv_turnpike(route: &Route) -> bool {
    let n = route.legs.len();
    if n == 0 {
        return false;
    }
    if route_has_forbidden_endpoint(route) {
        return false;
    }
    route.legs.iter().enumerate().all(|(i, leg)| {
        if leg_on_lcv_turnpike(leg) {
            return true;
        }
        if leg.a == leg.b {
            return true;
        }
        let at_end = i == 0 || i + 1 == n;
        at_end && leg_is_staging_access(leg)
    })
}

/// Keep only routes long doubles may run on the turnpike approximation.
pub fn filter_lcv_turnpike_routes(routes: &[Route]) -> Vec<Route> {
    routes
        .iter()
        .filter(|route| route_allows_lcv_turnpike(route))
        .cloned()
        .collect()
}

/// Whether a cargo class must stay on permitted LCV turnpike corridors.
pub fn cargo_requires_lcv_turnpike(cargo_key: &str) -> bool {
    cargo_key == "turnpike_doubles"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::world_models::Leg;

    fn ohio_west_turnpike_leg() -> Leg {
        Leg::new(
            "toledo_oh_us",
            "elkhart_in_us",
            139.0,
            "I-80",
            "flat",
            Vec::new(),
        )
    }

    fn plain_interstate_leg() -> Leg {
        Leg::new(
            "columbus_oh_us",
            "cincinnati_oh_us",
            100.0,
            "I-71",
            "flat",
            Vec::new(),
        )
    }

    #[test]
    fn cue_d_end_leg_counts_as_staging_only_up_to_one_mile() {
        let cued = |miles: f64| {
            let mut leg = Leg::new(
                "sandusky_oh_us",
                "toledo_oh_us",
                miles,
                "Dock Spur",
                "flat",
                Vec::new(),
            );
            leg.local_cue = "right onto Dock Spur".to_string();
            leg
        };
        let route_with = |first: Leg| {
            Route::from_legs(
                vec![
                    "sandusky_oh_us".into(),
                    "toledo_oh_us".into(),
                    "elkhart_in_us".into(),
                ],
                vec![first, ohio_west_turnpike_leg()],
            )
        };

        let long = cued(LCV_STAGING_ACCESS_MAX_MI + 0.5);
        assert!(!leg_is_staging_access(&long));
        assert!(!route_allows_lcv_turnpike(&route_with(long)));

        let at_cap = cued(LCV_STAGING_ACCESS_MAX_MI);
        assert!(leg_is_staging_access(&at_cap));
        assert!(route_allows_lcv_turnpike(&route_with(at_cap)));

        let short = cued(0.4);
        assert!(leg_is_staging_access(&short));
        assert!(route_allows_lcv_turnpike(&route_with(short)));

        // The same cap holds on the last leg, off the turnpike to the dock.
        let cued_off = |miles: f64| {
            let mut leg = Leg::new(
                "elkhart_in_us",
                "south_bend_in_us",
                miles,
                "Dock Spur",
                "flat",
                Vec::new(),
            );
            leg.local_cue = "left onto Dock Spur".to_string();
            leg
        };
        let route_ending = |last: Leg| {
            Route::from_legs(
                vec![
                    "toledo_oh_us".into(),
                    "elkhart_in_us".into(),
                    "south_bend_in_us".into(),
                ],
                vec![ohio_west_turnpike_leg(), last],
            )
        };
        let long_last = cued_off(LCV_STAGING_ACCESS_MAX_MI + 0.5);
        assert!(!leg_is_staging_access(&long_last));
        assert!(!route_allows_lcv_turnpike(&route_ending(long_last)));
        let last_at_cap = cued_off(LCV_STAGING_ACCESS_MAX_MI);
        assert!(leg_is_staging_access(&last_at_cap));
        assert!(route_allows_lcv_turnpike(&route_ending(last_at_cap)));
    }

    #[test]
    fn allowed_corridor_legs_match_city_pairs() {
        assert!(leg_on_lcv_turnpike(&ohio_west_turnpike_leg()));
        assert!(leg_on_lcv_turnpike(&Leg::new(
            "buffalo_ny_us",
            "syracuse_ny_us",
            149.0,
            "I-90",
            "flat",
            Vec::new(),
        )));
        assert!(leg_on_lcv_turnpike(&Leg::new(
            "emporia_ks_us",
            "wichita_ks_us",
            90.0,
            "I-35",
            "flat",
            Vec::new(),
        )));
        assert!(!leg_on_lcv_turnpike(&plain_interstate_leg()));
        // Same cities on the wrong highway never count.
        assert!(!leg_on_lcv_turnpike(&Leg::new(
            "toledo_oh_us",
            "elkhart_in_us",
            139.0,
            "I-90",
            "flat",
            Vec::new(),
        )));
    }

    #[test]
    fn ohio_i90_east_of_elyria_and_cleveland_are_refused() {
        // I-90 Cleveland ↔ Toledo spans the Elyria split into Cleveland.
        assert!(!leg_on_lcv_turnpike(&Leg::new(
            "cleveland_oh_us",
            "toledo_oh_us",
            114.0,
            "I-90",
            "flat",
            Vec::new(),
        )));
        // I-90 Erie ↔ Cleveland is east of Elyria (Cleveland and east).
        assert!(!leg_on_lcv_turnpike(&Leg::new(
            "erie_pa_us",
            "cleveland_oh_us",
            102.0,
            "I-90",
            "flat",
            Vec::new(),
        )));
        assert!(!route_allows_lcv_turnpike(&Route::from_legs(
            vec!["cleveland_oh_us".into(), "toledo_oh_us".into()],
            vec![Leg::new(
                "cleveland_oh_us",
                "toledo_oh_us",
                114.0,
                "I-90",
                "flat",
                Vec::new(),
            )],
        )));
    }

    #[test]
    fn kansas_i70_west_of_topeka_is_refused() {
        assert!(!leg_on_lcv_turnpike(&Leg::new(
            "topeka_ks_us",
            "junction_city_ks_us",
            65.0,
            "I-70",
            "flat",
            Vec::new(),
        )));
        assert!(!leg_on_lcv_turnpike(&Leg::new(
            "salina_ks_us",
            "hays_ks_us",
            98.0,
            "I-70",
            "flat",
            Vec::new(),
        )));
        assert!(!leg_on_lcv_turnpike(&Leg::new(
            "colby_ks_us",
            "burlington_co_us",
            69.0,
            "I-70",
            "flat",
            Vec::new(),
        )));
    }

    #[test]
    fn buffalo_to_erie_is_refused() {
        // Thruway ends at the PA line; Pennsylvania allows no turnpike doubles.
        let leg = Leg::new(
            "buffalo_ny_us",
            "erie_pa_us",
            94.0,
            "I-90",
            "flat",
            Vec::new(),
        );
        assert!(!leg_on_lcv_turnpike(&leg));
        assert!(!route_allows_lcv_turnpike(&Route::from_legs(
            vec!["buffalo_ny_us".into(), "erie_pa_us".into()],
            vec![leg],
        )));
    }

    #[test]
    fn nyc_endpoints_are_refused_for_turnpike_doubles() {
        // TAP-602: tandems stay on the Thruway; Exit 6A Yonkers is the
        // southern lot -- NYC docks are not reachable by end-stub or
        // same-city staging. The coarse I-87 new_york–albany world leg is
        // not on the allowlist.
        assert!(!city_allows_lcv_turnpike_endpoint("new_york_ny_us"));
        assert!(city_allows_lcv_turnpike_endpoint("albany_ny_us"));
        let i87 = Leg::new(
            "new_york_ny_us",
            "albany_ny_us",
            145.0,
            "I-87",
            "flat",
            Vec::new(),
        );
        assert!(!leg_on_lcv_turnpike(&i87));
        assert!(!route_allows_lcv_turnpike(&Route::from_legs(
            vec!["new_york_ny_us".into(), "albany_ny_us".into()],
            vec![i87],
        )));
        assert!(!route_allows_lcv_turnpike(&Route::from_legs(
            vec!["albany_ny_us".into(), "new_york_ny_us".into()],
            vec![Leg::new(
                "albany_ny_us",
                "new_york_ny_us",
                145.0,
                "I-87",
                "flat",
                Vec::new(),
            )],
        )));
        // Same-city NYC stub alone never clears the gate.
        let stub = Leg::local(
            "new_york_ny_us",
            0.4,
            "Dock Road",
            "right onto Dock Road",
            25.0,
        );
        assert!(!route_allows_lcv_turnpike(&Route::from_legs(
            vec!["new_york_ny_us".into(), "new_york_ny_us".into()],
            vec![stub],
        )));
    }

    #[test]
    fn gvw_caps_are_recorded_for_fix_five() {
        assert_eq!(lcv_turnpike_gvw_cap_lb("OH"), Some(127_400));
        assert_eq!(lcv_turnpike_gvw_cap_lb("IN"), Some(127_400));
        assert_eq!(lcv_turnpike_gvw_cap_lb("MA"), Some(127_400));
        assert_eq!(lcv_turnpike_gvw_cap_lb("NY"), Some(143_000));
        assert_eq!(lcv_turnpike_gvw_cap_lb("KS"), Some(120_000));
        assert_eq!(lcv_turnpike_gvw_cap_lb("FL"), None);
        assert_eq!(lcv_turnpike_gvw_cap_lb("CA"), None);
    }

    #[test]
    fn non_permitted_road_route_is_refused() {
        let route = Route::from_legs(
            vec!["columbus_oh_us".into(), "cincinnati_oh_us".into()],
            vec![plain_interstate_leg()],
        );
        assert!(!route_allows_lcv_turnpike(&route));
    }

    #[test]
    fn turnpike_route_with_staging_stub_is_allowed() {
        let stub = Leg::local(
            "toledo_oh_us",
            0.6,
            "Staging Lot Road",
            "right onto Staging Lot Road",
            25.0,
        );
        let route = Route::from_legs(
            vec![
                "toledo_oh_us".into(),
                "toledo_oh_us".into(),
                "elkhart_in_us".into(),
            ],
            vec![stub, ohio_west_turnpike_leg()],
        );
        assert!(route_allows_lcv_turnpike(&route));
    }

    #[test]
    fn mid_route_connector_never_counts_as_staging() {
        let route = Route::from_legs(
            vec![
                "toledo_oh_us".into(),
                "mid_oh_us".into(),
                "elmore_oh_us".into(),
                "elkhart_in_us".into(),
            ],
            vec![
                Leg::new(
                    "toledo_oh_us",
                    "mid_oh_us",
                    40.0,
                    "I-80",
                    "flat",
                    Vec::new(),
                ),
                Leg::new(
                    "mid_oh_us",
                    "elmore_oh_us",
                    0.5,
                    "Frontage Rd",
                    "flat",
                    Vec::new(),
                ),
                Leg::new(
                    "elmore_oh_us",
                    "elkhart_in_us",
                    40.0,
                    "I-80",
                    "flat",
                    Vec::new(),
                ),
            ],
        );
        // Middle leg is not an allowlisted city pair and is not an end stub.
        assert!(!route_allows_lcv_turnpike(&route));
    }

    #[test]
    fn filter_keeps_turnpike_options_only() {
        let ok = Route::from_legs(
            vec!["toledo_oh_us".into(), "elkhart_in_us".into()],
            vec![ohio_west_turnpike_leg()],
        );
        let bad = Route::from_legs(
            vec!["columbus_oh_us".into(), "cincinnati_oh_us".into()],
            vec![plain_interstate_leg()],
        );
        let kept = filter_lcv_turnpike_routes(&[bad, ok.clone()]);
        assert_eq!(kept.len(), 1);
        assert!(route_allows_lcv_turnpike(&kept[0]));
    }

    #[test]
    fn only_turnpike_doubles_requires_the_turnpike() {
        assert!(cargo_requires_lcv_turnpike("turnpike_doubles"));
        assert!(!cargo_requires_lcv_turnpike("parcel_doubles"));
        assert!(!cargo_requires_lcv_turnpike("general"));
    }

    #[test]
    fn refusal_line_is_distinct_from_staa_lines() {
        use crate::data::national_network::{
            STAA_DOUBLES_CORRIDOR_REFUSAL, STAA_DOUBLES_ROUTE_REFUSAL,
        };
        assert_eq!(
            LCV_TURNPIKE_ROUTE_REFUSAL,
            "Dispatch only clears long doubles on the turnpike."
        );
        assert_ne!(LCV_TURNPIKE_ROUTE_REFUSAL, STAA_DOUBLES_ROUTE_REFUSAL);
        assert_ne!(LCV_TURNPIKE_ROUTE_REFUSAL, STAA_DOUBLES_CORRIDOR_REFUSAL);
    }
}
