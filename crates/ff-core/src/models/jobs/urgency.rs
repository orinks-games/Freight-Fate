//! How fast the shipper needs a load: hot, standard or flexible.
//!
//! Drawn from the load itself ([`load_seed`](crate::models::co_driver::load_seed)),
//! not from the board's dice, so adding it left every board posting the same
//! loads. A hot load pays more and allows less time; a flexible delivery pays
//! less and allows more.
//!
//! Provenance: every number here is **assumed**. A search on 2026-11-21 for a
//! published expedited-truckload premium and for the share of time-critical
//! truckload freight found carrier and broker marketing pages, no survey or
//! index a constant could cite. The shares are chosen so the average pay does
//! not move (0.15 * 1.25 + 0.30 * 0.90 + 0.55 = 1.0075), and the slack ranges
//! so no deadline is tighter than the game already allowed.

use crate::pyrandom::PyRandom;

/// **Assumed**: the share of loads posted hot.
pub const HOT_LOAD_SHARE: f64 = 0.15;
/// **Assumed**: the share of loads posted as flexible deliveries.
pub const FLEXIBLE_LOAD_SHARE: f64 = 0.30;
/// **Assumed**: a hot load's pay over the same load posted standard.
pub const HOT_LOAD_PAY_MULT: f64 = 1.25;
/// **Assumed**: a flexible delivery's pay against the same load standard.
pub const FLEXIBLE_LOAD_PAY_MULT: f64 = 0.90;
/// Shipper slack on a hot load: the tight end of the old 1.2 to 1.5 range.
pub const HOT_LOAD_SLACK: f64 = 1.2;
/// **Assumed**: shipper slack on a standard load.
pub const STANDARD_LOAD_SLACK_RANGE: (f64, f64) = (1.3, 1.5);
/// **Assumed**: shipper slack on a flexible delivery.
pub const FLEXIBLE_LOAD_SLACK_RANGE: (f64, f64) = (1.6, 2.0);

/// How fast the shipper needs a load.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum LoadUrgency {
    #[default]
    Standard,
    Hot,
    Flexible,
}

impl LoadUrgency {
    /// The urgency dispatch posts this load at; `seed` is the load's
    /// `load_seed`, so the board, the shipper and a resumed trip agree.
    pub fn drawn(seed: &str) -> Self {
        let roll = PyRandom::new_from_str(&format!("urgency:{seed}")).random();
        if roll < HOT_LOAD_SHARE {
            LoadUrgency::Hot
        } else if roll < HOT_LOAD_SHARE + FLEXIBLE_LOAD_SHARE {
            LoadUrgency::Flexible
        } else {
            LoadUrgency::Standard
        }
    }

    pub fn pay_mult(self) -> f64 {
        match self {
            LoadUrgency::Standard => 1.0,
            LoadUrgency::Hot => HOT_LOAD_PAY_MULT,
            LoadUrgency::Flexible => FLEXIBLE_LOAD_PAY_MULT,
        }
    }

    /// The shipper slack for this urgency from the board's one slack draw
    /// `draw`, which lies in `range` (the old `DEADLINE_DISPATCH_SLACK_RANGE`):
    /// the same draw lands at the same place in the class's own range.
    pub fn slack(self, draw: f64, range: (f64, f64)) -> f64 {
        let span = range.1 - range.0;
        let t = if span > 0.0 {
            ((draw - range.0) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let lerp = |(lo, hi): (f64, f64)| lo + (hi - lo) * t;
        match self {
            LoadUrgency::Hot => HOT_LOAD_SLACK,
            LoadUrgency::Standard => lerp(STANDARD_LOAD_SLACK_RANGE),
            LoadUrgency::Flexible => lerp(FLEXIBLE_LOAD_SLACK_RANGE),
        }
    }

    /// The save payload's word for it.
    pub fn key(self) -> &'static str {
        match self {
            LoadUrgency::Standard => "standard",
            LoadUrgency::Hot => "hot",
            LoadUrgency::Flexible => "flexible",
        }
    }

    /// Anything but "hot" or "flexible", a missing field included, reads as
    /// standard, so a save from before urgency loads unchanged.
    pub fn from_key(key: &str) -> Self {
        match key {
            "hot" => LoadUrgency::Hot,
            "flexible" => LoadUrgency::Flexible,
            _ => LoadUrgency::Standard,
        }
    }

    /// The board row's sentence; a standard load says nothing.
    pub fn board_sentence(self) -> &'static str {
        match self {
            LoadUrgency::Standard => "",
            LoadUrgency::Hot => "Hot load.",
            LoadUrgency::Flexible => "Flexible delivery.",
        }
    }

    /// The job details line; a standard load has none.
    pub fn details_line(self) -> &'static str {
        match self {
            LoadUrgency::Standard => "",
            LoadUrgency::Hot => "Hot load. Tight deadline, pays 25 percent more.",
            LoadUrgency::Flexible => "Flexible delivery. Wide deadline, pays 10 percent less.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::jobs::DEADLINE_DISPATCH_SLACK_RANGE;

    #[test]
    fn shares_follow_the_constants() {
        let n = 20_000;
        let (mut hot, mut flexible) = (0, 0);
        for i in 0..n {
            match LoadUrgency::drawn(&format!("a>b:general:{i}.0")) {
                LoadUrgency::Hot => hot += 1,
                LoadUrgency::Flexible => flexible += 1,
                LoadUrgency::Standard => {}
            }
        }
        let share = |c: i32| f64::from(c) / f64::from(n);
        assert!((share(hot) - HOT_LOAD_SHARE).abs() < 0.01, "{}", share(hot));
        assert!(
            (share(flexible) - FLEXIBLE_LOAD_SHARE).abs() < 0.01,
            "{}",
            share(flexible)
        );
    }

    #[test]
    fn the_same_load_draws_the_same_urgency() {
        let seed = "chicago_il_us>denver_co_us:general:20.4";
        assert_eq!(LoadUrgency::drawn(seed), LoadUrgency::drawn(seed));
    }

    #[test]
    fn slack_maps_the_draw_into_each_class() {
        let range = DEADLINE_DISPATCH_SLACK_RANGE;
        for (draw, standard, flexible) in [(1.2, 1.3, 1.6), (1.35, 1.4, 1.8), (1.5, 1.5, 2.0)] {
            assert!((LoadUrgency::Hot.slack(draw, range) - 1.2).abs() < 1e-9);
            assert!((LoadUrgency::Standard.slack(draw, range) - standard).abs() < 1e-9);
            assert!((LoadUrgency::Flexible.slack(draw, range) - flexible).abs() < 1e-9);
        }
    }

    #[test]
    fn no_class_is_tighter_than_the_old_range() {
        let range = DEADLINE_DISPATCH_SLACK_RANGE;
        for urgency in [
            LoadUrgency::Hot,
            LoadUrgency::Standard,
            LoadUrgency::Flexible,
        ] {
            assert!(urgency.slack(range.0, range) >= range.0);
        }
    }

    #[test]
    fn unknown_keys_read_as_standard() {
        for urgency in [
            LoadUrgency::Hot,
            LoadUrgency::Standard,
            LoadUrgency::Flexible,
        ] {
            assert_eq!(LoadUrgency::from_key(urgency.key()), urgency);
        }
        assert_eq!(LoadUrgency::from_key(""), LoadUrgency::Standard);
        assert_eq!(LoadUrgency::from_key("rush"), LoadUrgency::Standard);
    }
}
