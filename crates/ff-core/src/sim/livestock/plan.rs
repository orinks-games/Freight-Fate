//! Planning a cattle run at the board: whether it fits inside the 28-hour
//! limit, and whether a solo driver can make it without a 10-hour break.
//!
//! Read: no 30-minute break is owed while livestock are aboard on an
//! interstate trip (49 CFR 395.1(v)), and the driver's hours do not count
//! within 150 air miles of either end (395.1(k)). Derived from those: a solo
//! driver can legally cover about 950 road miles with no 10-hour break,
//! 173 miles, then 11 hours of driving, then 173 more.
//!
//! A carrier plans a long haul to finish inside the limit, because the feed,
//! water and rest stations that once lined the routes have nearly all gone
//! (from about 900 to five by 2017, Animal Welfare Institute). So the board
//! offers only runs that fit with room to spare, books a longer one for a
//! team, and never plans a 10-hour break with cattle aboard.

use super::exemption::EXEMPT_RADIUS_MI;
use super::limit::LIMIT_HOURS;
use super::words::CattleKind;

/// Hours the plan keeps in hand under the limit.
pub const CATTLE_RUN_MARGIN_H: f64 = 2.0;
/// Fuel and the unexpected, as the ordinary deadline allows.
const CATTLE_RUN_FLAT_H: f64 = 1.0;
/// The ordinary US driving limit between the circles.
const SOLO_DRIVING_LIMIT_H: f64 = 11.0;

/// Which facility types a cattle load runs between: calves from a sale barn
/// to a feed yard, and fed cattle from a feed yard to a beef plant.
pub fn livestock_lane_ok(origin_type: &str, destination_type: &str) -> bool {
    matches!(
        (origin_type, destination_type),
        ("sale_barn", "feed_yard") | ("feed_yard", "beef_plant")
    )
}

/// A cattle run as dispatch plans it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CattleRun {
    /// Driving hours the hours of service count: the share of the run
    /// outside both circles.
    pub counted_drive_h: f64,
    /// The whole run, loaded departure to the receiver.
    pub total_h: f64,
    /// A solo driver would owe a 10-hour break, so it is booked for a team.
    pub team: bool,
    /// The run finishes inside the limit with the margin in hand.
    pub fits: bool,
}

impl CattleRun {
    /// The delivery appointment, never later than the limit.
    pub fn deadline_h(&self, slack: f64) -> f64 {
        (self.total_h * slack + CATTLE_RUN_FLAT_H).min(LIMIT_HOURS)
    }
}

/// Plan a run of `drive_h` driving hours whose ends lie `air_mi` apart in a
/// straight line. Fed cattle go to a plant inside one circle, so a fed-cattle
/// run that leaves it is no run at all.
pub fn cattle_run(kind: CattleKind, drive_h: f64, air_mi: f64) -> CattleRun {
    let drive_h = drive_h.max(0.0);
    let air_mi = air_mi.max(0.0);
    // Derived: the straight line's share outside both circles stands for the
    // road's share.
    let counted_share = if air_mi <= 2.0 * EXEMPT_RADIUS_MI || air_mi <= 0.0 {
        0.0
    } else {
        (air_mi - 2.0 * EXEMPT_RADIUS_MI) / air_mi
    };
    let counted_drive_h = drive_h * counted_share;
    let total_h = drive_h + CATTLE_RUN_FLAT_H;
    let short_enough = match kind {
        CattleKind::FedCattle => air_mi <= EXEMPT_RADIUS_MI,
        CattleKind::FeederCalves => true,
    };
    CattleRun {
        counted_drive_h,
        total_h,
        team: counted_drive_h > SOLO_DRIVING_LIMIT_H,
        fits: short_enough && total_h + CATTLE_RUN_MARGIN_H <= LIMIT_HOURS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_two_cattle_lanes_are_booked() {
        assert!(livestock_lane_ok("sale_barn", "feed_yard"));
        assert!(livestock_lane_ok("feed_yard", "beef_plant"));
        assert!(!livestock_lane_ok("sale_barn", "beef_plant"));
        assert!(!livestock_lane_ok("feed_yard", "feed_yard"));
        assert!(!livestock_lane_ok("beef_plant", "feed_yard"));
        assert!(!livestock_lane_ok("dry_warehouse", "feed_yard"));
    }

    #[test]
    fn a_run_inside_the_circles_never_counts_the_drivers_hours() {
        let run = cattle_run(CattleKind::FedCattle, 2.5, 120.0);
        assert_eq!(run.counted_drive_h, 0.0);
        assert!(!run.team);
        assert!(run.fits);
        let overlap = cattle_run(CattleKind::FeederCalves, 6.0, 340.0);
        assert_eq!(overlap.counted_drive_h, 0.0);
    }

    #[test]
    fn fed_cattle_stay_inside_one_circle() {
        assert!(!cattle_run(CattleKind::FedCattle, 4.0, 200.0).fits);
    }

    #[test]
    fn about_950_road_miles_is_the_longest_solo_run() {
        // 950 road miles at 55 an hour, about 820 in a straight line.
        let solo = cattle_run(CattleKind::FeederCalves, 950.0 / 55.0, 820.0);
        assert!(!solo.team, "{solo:?}");
        assert!(solo.fits);
        let team = cattle_run(CattleKind::FeederCalves, 1_250.0 / 55.0, 1_080.0);
        assert!(team.team, "{team:?}");
        assert!(team.fits);
        let too_far = cattle_run(CattleKind::FeederCalves, 1_500.0 / 55.0, 1_300.0);
        assert!(!too_far.fits);
    }

    #[test]
    fn the_appointment_never_falls_past_the_limit() {
        let run = cattle_run(CattleKind::FeederCalves, 22.0, 1_000.0);
        assert!(run.deadline_h(1.5) <= LIMIT_HOURS);
        let short = cattle_run(CattleKind::FedCattle, 2.0, 100.0);
        assert!((short.deadline_h(1.2) - (3.0 * 1.2 + 1.0)).abs() < 1e-9);
    }
}
