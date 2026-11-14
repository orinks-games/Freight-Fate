//! The safety record: how interesting this driver looks to an inspector
//! (port of `freight_fate/models/safety_record.py`).
//!
//! Real commercial enforcement does not pick trucks at random. Every carrier
//! carries a score built from its inspection and violation history, and the
//! screening software at a scale reads it before anyone looks at the truck. A
//! clean operator is waved through year after year; a dirty one is pulled in
//! every time, which feels less like bad luck than like being followed.
//!
//! That is the dial this file provides, and it is what makes a clean career
//! pleasant and a dirty one relentless -- without the game having to cheat, and
//! without a clean driver's police contacts ever costing them anything.
//!
//! **Spoken as "safety record". Never as ISS, never as CSA, never as a score.**
//! Those are trade acronyms; a player hearing "your ISS is 78" learns nothing.
//! The number is internal. What the game says out loud is a band.
//!
//! Higher is worse, matching the real convention: 0 is a driver nobody has any
//! reason to look at, 100 is one every screening lane flags.
//!
//! The real score weighs the last 24 months, so a bad stretch fades. Here the
//! citations, serious violations, out-of-service orders and fatigue events
//! count inside [`SAFETY_RECORD_WINDOW_DAYS`] (serious violations and crashes
//! inside [`SAFETY_RECORD_SERIOUS_WINDOW_DAYS`]), the windows reputation and
//! the carrier's review already use. Clean inspections stay a lifetime credit.

use crate::models::enforcement::{REVIEW_WINDOW_DAYS, SERIOUS_REVIEW_WINDOW_DAYS};
use crate::pyfmt::round_py_n;

/// How far back the scale's screening looks. The real carrier score is
/// time-weighted over 24 months; this is the game's review window for the
/// same reason `REVIEW_WINDOW_DAYS` gives: the clock only moves on the road,
/// so a longer window is a lifetime mark by another name (owner, 2026-09-14).
pub const SAFETY_RECORD_WINDOW_DAYS: i64 = REVIEW_WINDOW_DAYS;
/// Serious violations and crashes count three times as long, as they do in
/// the carrier's review.
pub const SAFETY_RECORD_SERIOUS_WINDOW_DAYS: i64 = SERIOUS_REVIEW_WINDOW_DAYS;

pub const SAFETY_RECORD_MIN: f64 = 0.0;
pub const SAFETY_RECORD_MAX: f64 = 100.0;

/// A brand-new career has no history at all. Neither clean nor dirty: unknown,
/// and unknown gets looked at a little more than proven-clean does.
pub const SAFETY_RECORD_BASELINE: f64 = 30.0;

// Band edges. "Targeted" sits at the real screening threshold.
pub const BAND_CLEAN_BELOW: f64 = 40.0;
pub const BAND_TARGETED_AT: f64 = 75.0;

pub const BAND_CLEAN: &str = "clean";
pub const BAND_WATCHED: &str = "watched";
pub const BAND_TARGETED: &str = "targeted";

// What each input is worth. Citations dominate, because a citation is a
// recorded finding; damage matters because it is visible from the shoulder;
// reputation pulls both ways because a carrier in good standing gets the
// benefit of the doubt.
pub const CITATION_WEIGHT: f64 = 7.0;
pub const CITATION_CAP: f64 = 35.0;
pub const SERIOUS_WEIGHT: f64 = 9.0;
pub const SERIOUS_CAP: f64 = 27.0;
pub const OUT_OF_SERVICE_WEIGHT: f64 = 12.0;
pub const OUT_OF_SERVICE_CAP: f64 = 24.0;
pub const FATIGUE_EVENT_WEIGHT: f64 = 6.0;
pub const FATIGUE_EVENT_CAP: f64 = 18.0;
/// Damage starts counting where an officer could see it from a passing lane.
pub const DAMAGE_NOTICE_PCT: f64 = 40.0;
pub const DAMAGE_WEIGHT: f64 = 0.45;
pub const DAMAGE_CAP: f64 = 27.0;
// A clean inspection is the only thing that buys the record back down, and it
// is worth less than a citation costs -- which is exactly how the real ratings
// behave, and why drivers guard a clean record rather than repairing one.
pub const CLEAN_INSPECTION_CREDIT: f64 = 3.5;
pub const CLEAN_INSPECTION_CAP: f64 = 21.0;
/// Reputation shifts the baseline by up to this much in either direction.
pub const REPUTATION_SWING: f64 = 12.0;
pub const REPUTATION_NEUTRAL: f64 = 50.0;

fn clamp(value: f64) -> f64 {
    value.clamp(SAFETY_RECORD_MIN, SAFETY_RECORD_MAX)
}

/// The keyword inputs of `selection_score(...)`; every field has the Python
/// default, so `..Default::default()` spells the omitted keywords.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectionInputs {
    pub reputation: f64,
    pub citations: i64,
    pub serious_violations: i64,
    pub out_of_service_events: i64,
    pub fatigue_events: i64,
    pub clean_inspections: i64,
    pub damage_pct: f64,
}

impl Default for SelectionInputs {
    fn default() -> Self {
        Self {
            reputation: REPUTATION_NEUTRAL,
            citations: 0,
            serious_violations: 0,
            out_of_service_events: 0,
            fatigue_events: 0,
            clean_inspections: 0,
            damage_pct: 0.0,
        }
    }
}

/// How interesting this driver looks to a screening lane, 0 to 100.
///
/// Pure: every input is passed in, so the same history always produces the
/// same number and the function is testable without a Profile.
pub fn selection_score(inputs: &SelectionInputs) -> f64 {
    let mut score = SAFETY_RECORD_BASELINE;
    score += CITATION_CAP.min(CITATION_WEIGHT * inputs.citations.max(0) as f64);
    score += SERIOUS_CAP.min(SERIOUS_WEIGHT * inputs.serious_violations.max(0) as f64);
    score +=
        OUT_OF_SERVICE_CAP.min(OUT_OF_SERVICE_WEIGHT * inputs.out_of_service_events.max(0) as f64);
    score += FATIGUE_EVENT_CAP.min(FATIGUE_EVENT_WEIGHT * inputs.fatigue_events.max(0) as f64);
    let visible_damage = (inputs.damage_pct - DAMAGE_NOTICE_PCT).max(0.0);
    score += DAMAGE_CAP.min(DAMAGE_WEIGHT * visible_damage);
    score -=
        CLEAN_INSPECTION_CAP.min(CLEAN_INSPECTION_CREDIT * inputs.clean_inspections.max(0) as f64);
    // Standing with the carrier and the shippers is not a safety rating, but a
    // driver nobody has a complaint about does get the benefit of the doubt.
    let rep_offset = (inputs.reputation - REPUTATION_NEUTRAL) / REPUTATION_NEUTRAL;
    score -= REPUTATION_SWING * rep_offset.clamp(-1.0, 1.0);
    round_py_n(clamp(score), 2)
}

/// What `score_for_profile` reads off a live Profile, defensively.
///
/// Every method has the default the Python `getattr(..., default)` used, so a
/// partially built profile, an older save and the test fixtures all work.
/// The four record counts are the ones inside [`SAFETY_RECORD_WINDOW_DAYS`].
pub trait SafetyRecordProfile {
    fn career_reputation(&self) -> f64 {
        REPUTATION_NEUTRAL
    }
    fn record_citations(&self) -> i64 {
        0
    }
    fn record_serious_violation_count(&self) -> i64 {
        0
    }
    fn out_of_service_events(&self) -> i64 {
        0
    }
    fn record_fatigue_events(&self) -> i64 {
        0
    }
    /// Crashes on the accident register inside the window.
    fn record_crashes(&self) -> i64 {
        0
    }
    /// `achievement_stats.get("inspections_passed", 0)`.
    fn inspections_passed(&self) -> i64 {
        0
    }
    /// `profile.selection_score = score`.
    fn set_selection_score(&mut self, score: f64);
}

/// The safety record for a live Profile, read defensively.
///
/// Every field is read with a default so this works against a partially
/// built profile, an older save, and the test fixtures -- and so a field a
/// parallel change is still adding cannot break the whole enforcement layer.
pub fn score_for_profile<P: SafetyRecordProfile + ?Sized>(profile: &P, damage_pct: f64) -> f64 {
    // Python reads `getattr(career, "reputation", NEUTRAL) or NEUTRAL`, so a
    // reputation of exactly zero reads as neutral there too.
    let reputation = profile.career_reputation();
    let reputation = if reputation == 0.0 {
        REPUTATION_NEUTRAL
    } else {
        reputation
    };
    selection_score(&SelectionInputs {
        reputation,
        citations: profile.record_citations(),
        // A crash on the accident register (49 CFR 390.15) weighs as a serious
        // event does (owner ruling, 2026-09-24).
        serious_violations: profile.record_serious_violation_count() + profile.record_crashes(),
        out_of_service_events: profile.out_of_service_events(),
        fatigue_events: profile.record_fatigue_events(),
        clean_inspections: profile.inspections_passed(),
        damage_pct,
    })
}

/// Recompute and store the record on the profile, returning it.
pub fn refresh_selection_score<P: SafetyRecordProfile + ?Sized>(
    profile: &mut P,
    damage_pct: f64,
) -> f64 {
    let score = score_for_profile(profile, damage_pct);
    profile.set_selection_score(score);
    score
}

pub fn safety_band(score: f64) -> &'static str {
    if score < BAND_CLEAN_BELOW {
        return BAND_CLEAN;
    }
    if score < BAND_TARGETED_AT {
        return BAND_WATCHED;
    }
    BAND_TARGETED
}

/// The spoken line. A band and a reason, never a number and never jargon.
pub fn safety_record_text(score: f64) -> &'static str {
    let band = safety_band(score);
    if band == BAND_CLEAN {
        return "Safety record: clean. Inspectors have no reason to pull you in.";
    }
    if band == BAND_WATCHED {
        return "Safety record: watched. Open scales will look at you more often than not.";
    }
    "Safety record: targeted. Expect to be pulled in at every open scale."
}

/// Odds an open scale sends this driver to the inspection lane.
///
/// Clean drivers are waved through nearly always; a targeted one is not
/// waved through at all. This is the whole difference between the pleasant
/// career and the relentless one.
pub fn inspection_selection_chance(score: f64) -> f64 {
    let band = safety_band(score);
    if band == BAND_CLEAN {
        return 0.08;
    }
    if band == BAND_WATCHED {
        return 0.45;
    }
    1.0
}

#[cfg(test)]
mod tests {
    //! Ported from the safety-record cases in `tests/test_enforcement_presence.py`.

    use super::*;

    #[test]
    fn test_a_clean_record_is_waved_through_and_a_dirty_one_is_not() {
        let clean = selection_score(&SelectionInputs {
            reputation: 80.0,
            clean_inspections: 6,
            ..Default::default()
        });
        let dirty = selection_score(&SelectionInputs {
            reputation: 20.0,
            citations: 4,
            serious_violations: 2,
            out_of_service_events: 2,
            damage_pct: 70.0,
            ..Default::default()
        });
        assert_eq!(safety_band(clean), "clean");
        assert_eq!(safety_band(dirty), "targeted");
        assert!(inspection_selection_chance(clean) < 0.15);
        assert_eq!(inspection_selection_chance(dirty), 1.0);
    }

    struct Fake {
        reputation: f64,
        citations: i64,
        selection_score: f64,
    }

    impl SafetyRecordProfile for Fake {
        fn career_reputation(&self) -> f64 {
            self.reputation
        }
        fn record_citations(&self) -> i64 {
            self.citations
        }
        fn set_selection_score(&mut self, score: f64) {
            self.selection_score = score;
        }
    }

    #[test]
    fn test_the_safety_record_rides_on_the_profile_and_survives_a_save() {
        // The pure half: three citations and 60 percent damage read over 40.
        // The Profile.from_dict round trip needs models::profile.
        let mut profile = Fake {
            reputation: 50.0,
            citations: 3,
            selection_score: SAFETY_RECORD_BASELINE,
        };
        refresh_selection_score(&mut profile, 60.0);
        assert!(profile.selection_score > 40.0);
        assert_eq!(profile.selection_score, score_for_profile(&profile, 60.0));
    }

    #[test]
    fn a_new_career_sits_at_the_baseline_and_zero_reputation_reads_neutral() {
        assert_eq!(
            selection_score(&SelectionInputs::default()),
            SAFETY_RECORD_BASELINE
        );
        let zero = Fake {
            reputation: 0.0,
            citations: 0,
            selection_score: 0.0,
        };
        // `getattr(career, "reputation", 50.0) or 50.0`: falsy zero is neutral.
        assert_eq!(score_for_profile(&zero, 0.0), SAFETY_RECORD_BASELINE);
        let low = Fake {
            reputation: 1.0,
            citations: 0,
            selection_score: 0.0,
        };
        assert!(score_for_profile(&low, 0.0) > SAFETY_RECORD_BASELINE);
    }

    #[test]
    fn the_spoken_line_is_a_band_never_jargon() {
        for score in [0.0, 39.9, 40.0, 74.9, 75.0, 100.0] {
            let line = safety_record_text(score);
            assert!(line.starts_with("Safety record:"));
            for jargon in ["ISS", "CSA", "SMS", "score"] {
                assert!(!line.contains(jargon), "{line}");
            }
        }
        assert_eq!(safety_band(39.99), BAND_CLEAN);
        assert_eq!(safety_band(40.0), BAND_WATCHED);
        assert_eq!(safety_band(75.0), BAND_TARGETED);
    }

    #[test]
    fn the_caps_hold_and_the_score_stays_in_range() {
        let worst = selection_score(&SelectionInputs {
            reputation: -500.0,
            citations: 999,
            serious_violations: 999,
            out_of_service_events: 999,
            fatigue_events: 999,
            damage_pct: 1000.0,
            ..Default::default()
        });
        assert_eq!(worst, SAFETY_RECORD_MAX);
        let best = selection_score(&SelectionInputs {
            reputation: 1000.0,
            clean_inspections: 999,
            ..Default::default()
        });
        assert_eq!(best, SAFETY_RECORD_MIN);
        // Negative counts read as zero, as `max(0, int(...))` does.
        let negatives = selection_score(&SelectionInputs {
            citations: -3,
            clean_inspections: -3,
            ..Default::default()
        });
        assert_eq!(negatives, SAFETY_RECORD_BASELINE);
    }
}
