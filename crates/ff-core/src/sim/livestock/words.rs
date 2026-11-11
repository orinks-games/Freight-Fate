//! The cattle themselves: what kind, how many, what the receiver says, and
//! the readout words for the condition meter.
//!
//! The game reports counts and outcomes in the words of a bill of lading and
//! describes no suffering.

use serde::{Deserialize, Serialize};

use crate::models::cargo_condition::{
    cargo_outcome, CARGO_CLAIM_PCT, CARGO_OUTCOME_CLAIM, CARGO_OUTCOME_EXCEPTION,
    CARGO_OUTCOME_REJECTED, CARGO_REJECT_PCT,
};

/// Pounds in a kilogram.
const LB_PER_KG: f64 = 2.204_622_6;

/// Which cattle a load carries, from where they were loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CattleKind {
    /// Calves bought at a sale barn, going to a feed yard: the long runs.
    FeederCalves,
    /// Finished cattle from a feed yard, going to a beef plant: short runs.
    FedCattle,
}

impl CattleKind {
    /// A sale barn ships feeder calves; anywhere else, fed cattle.
    pub fn for_origin(facility_type: &str) -> CattleKind {
        if facility_type == "sale_barn" {
            CattleKind::FeederCalves
        } else {
            CattleKind::FedCattle
        }
    }

    /// Assumed weights for each animal, from the trade's usual figures: a
    /// feeder calf about 550 pounds, a finished steer about 1,400.
    pub fn pounds_each(self) -> f64 {
        match self {
            CattleKind::FeederCalves => 550.0,
            CattleKind::FedCattle => 1_400.0,
        }
    }

    pub fn noun(self) -> &'static str {
        match self {
            CattleKind::FeederCalves => "feeder calves",
            CattleKind::FedCattle => "fed cattle",
        }
    }

    /// Head on a load of this many tonnes, at least 1.
    pub fn head_for_tons(self, weight_tons: f64) -> u32 {
        let pounds = weight_tons.max(0.0) * 1000.0 * LB_PER_KG;
        ((pounds / self.pounds_each()).round() as u32).max(1)
    }

    pub fn key(self) -> &'static str {
        match self {
            CattleKind::FeederCalves => "feeder_calves",
            CattleKind::FedCattle => "fed_cattle",
        }
    }
}

/// "89 head of feeder calves".
pub fn head_text(head: u32, kind: CattleKind) -> String {
    format!("{head} head of {}", kind.noun())
}

/// The load's weight in whole pounds, the way a scale ticket reads.
pub fn pounds(weight_tons: f64) -> f64 {
    (weight_tons.max(0.0) * 1000.0 * LB_PER_KG).round()
}

/// Read: Ohio State's shrink table, weight lost against hours on a moving
/// truck: 2 percent in the first hour, 4 to 6 by 8 hours, 6 to 8 by 16,
/// 8 to 10 by 24 and 10 to 12 by 32. Derived: a straight line through the
/// low end of each band.
const SHRINK_TABLE: &[(f64, f64)] = &[
    (0.0, 0.0),
    (1.0, 2.0),
    (2.0, 4.0),
    (8.0, 6.0),
    (16.0, 8.0),
    (24.0, 10.0),
    (32.0, 12.0),
];

/// Shrink, in percent of live weight, after this many hours aboard.
pub fn shrink_pct(hours_aboard: f64) -> f64 {
    let hours = hours_aboard.max(0.0);
    for pair in SHRINK_TABLE.windows(2) {
        let (h0, s0) = pair[0];
        let (h1, s1) = pair[1];
        if hours <= h1 {
            return s0 + (s1 - s0) * (hours - h0) / (h1 - h0);
        }
    }
    let (h_last, s_last) = SHRINK_TABLE[SHRINK_TABLE.len() - 1];
    // Past the table, the last band's rate: a quarter percent an hour.
    s_last + 0.25 * (hours - h_last)
}

/// The cargo meter's rungs in the words that fit live cattle. Same
/// thresholds and the same money as boxed freight and tanks.
pub const LIVESTOCK_CONDITION_TEXT: &[(&str, &str)] = &[
    (CARGO_OUTCOME_REJECTED, "lost"),
    (CARGO_OUTCOME_CLAIM, "injured"),
    (CARGO_OUTCOME_EXCEPTION, "bruised"),
];

/// How many head the receiver counts as dead or unable to walk off, for a
/// condition past the claim line. Derived from the meter so the count grows
/// with it; at least 1 once the line is crossed.
fn head_hurt(condition_pct: f64, head: u32, from_pct: f64, per_head_pct: f64) -> u32 {
    let n = 1 + ((condition_pct - from_pct).max(0.0) / per_head_pct).floor() as u32;
    n.min(head.max(1))
}

/// What the receiver says when the count is done.
pub fn receiver_ruling_line(
    condition_pct: f64,
    head: u32,
    hours_aboard: f64,
    one_down: bool,
) -> String {
    match cargo_outcome(condition_pct) {
        CARGO_OUTCOME_REJECTED => {
            let dead = head_hurt(condition_pct, head, CARGO_REJECT_PCT, 10.0);
            format!("{dead} dead on arrival. The owner refuses to pay for the haul.")
        }
        CARGO_OUTCOME_CLAIM => {
            let hurt = head_hurt(condition_pct, head, CARGO_CLAIM_PCT, 12.0);
            if hurt == 1 {
                "1 could not walk off. The carrier pays a claim.".to_string()
            } else {
                format!("{hurt} could not walk off. The carrier pays a claim.")
            }
        }
        CARGO_OUTCOME_EXCEPTION => "Bruising noted on the bill of lading.".to_string(),
        _ => {
            let shrink = shrink_pct(hours_aboard).round();
            let hours = hours_aboard.round().max(1.0);
            let hours_word = if hours == 1.0 { "hour" } else { "hours" };
            let walking = if one_down {
                format!("{head} head off, 1 helped to its feet")
            } else {
                format!("{head} head off, all walking")
            };
            format!("{walking}. {hours:.0} {hours_word} aboard, shrink about {shrink:.0} percent.")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_pot_carries_the_trade_counts() {
        // About 49,000 pounds of cattle on 80,000 gross: 89 calves or 35
        // fed steers.
        let tons = 49_000.0 / LB_PER_KG / 1000.0;
        assert_eq!(CattleKind::FeederCalves.head_for_tons(tons), 89);
        assert_eq!(CattleKind::FedCattle.head_for_tons(tons), 35);
        assert_eq!(CattleKind::FedCattle.head_for_tons(0.0), 1);
    }

    #[test]
    fn the_origin_decides_the_cattle() {
        assert_eq!(
            CattleKind::for_origin("sale_barn"),
            CattleKind::FeederCalves
        );
        assert_eq!(CattleKind::for_origin("feed_yard"), CattleKind::FedCattle);
    }

    #[test]
    fn shrink_follows_the_ohio_state_bands() {
        assert_eq!(shrink_pct(0.0), 0.0);
        assert!((shrink_pct(1.0) - 2.0).abs() < 1e-9);
        assert!((4.0..=6.0).contains(&shrink_pct(5.0)));
        assert!((6.0..=8.0).contains(&shrink_pct(12.0)));
        assert!((8.0..=10.0).contains(&shrink_pct(20.0)));
        assert!((10.0..=12.0).contains(&shrink_pct(28.0)));
        assert!(shrink_pct(40.0) > shrink_pct(32.0));
    }

    #[test]
    fn the_receiver_counts_in_the_words_of_a_bill_of_lading() {
        assert_eq!(
            receiver_ruling_line(0.0, 89, 12.2, false),
            "89 head off, all walking. 12 hours aboard, shrink about 7 percent."
        );
        assert_eq!(
            receiver_ruling_line(0.5, 35, 0.6, true),
            "35 head off, 1 helped to its feet. 1 hour aboard, shrink about 1 percent."
        );
        assert_eq!(
            receiver_ruling_line(20.0, 89, 10.0, false),
            "Bruising noted on the bill of lading."
        );
        assert_eq!(
            receiver_ruling_line(36.0, 89, 10.0, false),
            "1 could not walk off. The carrier pays a claim."
        );
        assert_eq!(
            receiver_ruling_line(52.0, 89, 10.0, false),
            "2 could not walk off. The carrier pays a claim."
        );
        assert_eq!(
            receiver_ruling_line(91.0, 89, 10.0, false),
            "4 dead on arrival. The owner refuses to pay for the haul."
        );
    }
}
