//! Alaska's 2026 spring-breakup axle restrictions.
//!
//! The leg windows are transcribed from DOT&PF Public Notices NR 223348,
//! NR 223422, NR 224009, CR 223273, CR 224048, and CR 224160. Restrictions
//! are percentages of legal axle load under 17 AAC 25.013(e), effective at
//! 08:00 local and lifted at 08:00 on the end date. Every career year replays
//! these 2026 month/day dates.
//!
//! Not modeled: Anchorage/Mat-Su area-wide limits on unlisted local roads
//! (there are no local streets in the game), the drive-axle provision for
//! approved longer combination vehicles (the notices name approved LCVs, but
//! the game does not model approved Alaska LCVs), and the seven-day en-route
//! exception. Loads dispatched before a window are approximated as not
//! re-checked.

use chrono::{Datelike, NaiveDate};

use crate::data::world_models::Route;
use crate::sim::season::day_of_year;
use crate::sim::vehicle::{TruckState, KG_PER_LB};

/// DOT&PF/17 AAC 25.013(e) legal axle limits used by the 2026 notices.
/// READ: DOT&PF spring-breakup public notices and 17 AAC 25.013(e).
pub const AK_SINGLE_AXLE_LB: f64 = 20_000.0;
/// DOT&PF/17 AAC 25.013(e) legal tandem axle limit used by the notices.
/// READ: DOT&PF spring-breakup public notices and 17 AAC 25.013(e).
pub const AK_TANDEM_AXLE_LB: f64 = 38_000.0;

/// A seasonal restriction on the unordered pair of endpoint city keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeasonalWeightRestriction {
    pub a: &'static str,
    pub b: &'static str,
    pub highway: &'static str,
    pub percent: u8,
    pub start: (u32, u32),
    pub end: (u32, u32),
}

/// DOT&PF 2026 spring-breakup restrictions mapped to Freight Fate legs.
pub const ALASKA_2026_SPRING_BREAKUP: &[SeasonalWeightRestriction] = &[
    SeasonalWeightRestriction {
        a: "valdez_ak_us",
        b: "glennallen_ak_us",
        highway: "Richardson Highway",
        percent: 85,
        start: (4, 13),
        end: (5, 29),
    },
    SeasonalWeightRestriction {
        a: "glennallen_ak_us",
        b: "delta_junction_ak_us",
        highway: "Richardson Highway",
        percent: 85,
        start: (4, 17),
        end: (5, 29),
    },
    SeasonalWeightRestriction {
        a: "delta_junction_ak_us",
        b: "fairbanks_ak_us",
        highway: "Richardson Highway",
        percent: 85,
        start: (4, 17),
        end: (5, 29),
    },
    SeasonalWeightRestriction {
        a: "delta_junction_ak_us",
        b: "tok_ak_us",
        highway: "Alaska Highway",
        percent: 85,
        start: (4, 17),
        end: (5, 29),
    },
    SeasonalWeightRestriction {
        a: "glennallen_ak_us",
        b: "tok_ak_us",
        highway: "Tok Cutoff",
        percent: 85,
        start: (4, 17),
        end: (5, 29),
    },
    SeasonalWeightRestriction {
        a: "glennallen_ak_us",
        b: "palmer_ak_us",
        highway: "Glenn Highway",
        percent: 85,
        start: (4, 17),
        end: (5, 29),
    },
    SeasonalWeightRestriction {
        a: "tok_ak_us",
        b: "whitehorse_yt_ca",
        highway: "Alaska Highway",
        percent: 85,
        start: (4, 17),
        end: (5, 29),
    },
    SeasonalWeightRestriction {
        a: "anchorage_ak_us",
        b: "seward_ak_us",
        highway: "Seward Highway",
        percent: 85,
        start: (4, 6),
        end: (6, 15),
    },
    SeasonalWeightRestriction {
        a: "anchorage_ak_us",
        b: "soldotna_ak_us",
        highway: "Sterling Highway",
        percent: 85,
        start: (4, 6),
        end: (6, 15),
    },
    SeasonalWeightRestriction {
        a: "seward_ak_us",
        b: "soldotna_ak_us",
        highway: "Sterling Highway",
        percent: 85,
        start: (4, 6),
        end: (6, 15),
    },
    SeasonalWeightRestriction {
        a: "soldotna_ak_us",
        b: "homer_ak_us",
        highway: "Sterling Highway",
        percent: 85,
        start: (4, 6),
        end: (6, 8),
    },
    SeasonalWeightRestriction {
        a: "soldotna_ak_us",
        b: "kenai_ak_us",
        highway: "Kenai Spur Highway",
        percent: 85,
        start: (4, 6),
        end: (6, 15),
    },
];

fn ordinal((month, day): (u32, u32)) -> f64 {
    f64::from(
        NaiveDate::from_ymd_opt(2001, month, day)
            .expect("restriction dates are valid in the fixed calendar year")
            .ordinal(),
    )
}

/// Find the restriction active on the unordered city pair at this calendar
/// time. The window includes its start at 08:00 and excludes its end at 08:00.
pub fn active_restriction(
    a: &str,
    b: &str,
    calendar_hours: f64,
) -> Option<&'static SeasonalWeightRestriction> {
    let now = day_of_year(calendar_hours);
    ALASKA_2026_SPRING_BREAKUP.iter().find(|restriction| {
        let same_pair = (restriction.a == a && restriction.b == b)
            || (restriction.a == b && restriction.b == a);
        let now = now + 1e-9;
        same_pair
            && now >= ordinal(restriction.start) + 8.0 / 24.0
            && now < ordinal(restriction.end) + 8.0 / 24.0
    })
}

/// The strictest active restriction along a route; equal limits keep the
/// first restricted segment in drive order.
pub fn strictest_on_route(
    route: &Route,
    calendar_hours: f64,
) -> Option<&'static SeasonalWeightRestriction> {
    let mut strictest = None;
    for pair in route.cities.windows(2) {
        let Some(restriction) = active_restriction(&pair[0], &pair[1], calendar_hours) else {
            continue;
        };
        if strictest
            .is_none_or(|current: &SeasonalWeightRestriction| restriction.percent < current.percent)
        {
            strictest = Some(restriction);
        }
    }
    strictest
}

/// Gross-weight cap for this truck under a seasonal axle percentage.
pub fn seasonal_gvw_cap_kg(truck: &TruckState, percent: u8) -> f64 {
    let legal = truck.trailer_set.legal_gvw_kg;
    let cap = if truck.trailer_set.is_doubles() {
        // Conservative stand-in: the axle model does not split doubles.
        legal * f64::from(percent) / 100.0
    } else {
        truck.gross_cap_for_axle_limits_kg(
            AK_SINGLE_AXLE_LB * f64::from(percent) / 100.0 * KG_PER_LB,
            AK_TANDEM_AXLE_LB * f64::from(percent) / 100.0 * KG_PER_LB,
        )
    };
    legal.min(cap)
}

#[cfg(test)]
mod tests {
    use chrono::{Datelike, NaiveDate};

    use super::*;
    use crate::data::world::get_world;

    fn hours(month: u32, day: u32, hour: f64) -> f64 {
        let ordinal = f64::from(
            NaiveDate::from_ymd_opt(2001, month, day)
                .expect("test date is valid")
                .ordinal(),
        );
        (ordinal - 80.0).rem_euclid(365.0) * 24.0 + hour
    }

    #[test]
    fn valdez_glennallen_window_is_half_open_and_unordered() {
        let start = hours(4, 13, 8.0);
        let end = hours(5, 29, 8.0);
        for (a, b) in [
            ("valdez_ak_us", "glennallen_ak_us"),
            ("glennallen_ak_us", "valdez_ak_us"),
        ] {
            assert!(active_restriction(a, b, start - 1.0 / 60.0).is_none());
            assert_eq!(active_restriction(a, b, start).unwrap().percent, 85);
            assert_eq!(
                active_restriction(a, b, end - 1.0 / 60.0).unwrap().percent,
                85
            );
            assert!(active_restriction(a, b, end).is_none());
        }
    }

    #[test]
    fn other_corridor_windows_match_the_notice_dates() {
        assert!(
            active_restriction("fairbanks_ak_us", "coldfoot_ak_us", hours(5, 1, 12.0)).is_none()
        );
        assert!(active_restriction("soldotna_ak_us", "homer_ak_us", hours(6, 8, 9.0)).is_none());
        assert_eq!(
            active_restriction("anchorage_ak_us", "soldotna_ak_us", hours(6, 10, 12.0))
                .unwrap()
                .percent,
            85
        );
        assert!(
            active_restriction("anchorage_ak_us", "soldotna_ak_us", hours(6, 15, 9.0)).is_none()
        );
    }

    #[test]
    fn route_uses_the_strictest_active_leg() {
        let world = get_world();
        let route = world
            .supported_route("anchorage_ak_us", "valdez_ak_us", None)
            .expect("supported route lookup")
            .expect("Anchorage to Valdez route");
        assert_eq!(
            strictest_on_route(&route, hours(5, 1, 12.0))
                .expect("the route crosses a restricted corridor")
                .percent,
            85
        );
        assert!(strictest_on_route(&route, hours(7, 1, 12.0)).is_none());
    }
}
