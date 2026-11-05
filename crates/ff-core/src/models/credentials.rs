//! The credential ladder: every license add-on a driver can train for,
//! in one catalog.
//!
//! Four rungs, modeled on the real credential structure of U.S. trucking
//! (49 CFR 383 endorsements and restrictions, 49 CFR 380 training rules,
//! TSA vetting for hazmat and ports):
//!
//! - **Training** (act one): removing a restriction a fresh CDL carries.
//!   The E restriction -- automatic transmissions only -- is removed by a
//!   skills retest in a manual truck (49 CFR 383.95(c)), not by a fee.
//! - **Certificates** (early-mid): carrier training, not CDL law. The
//!   carrier sponsors each at its listed level once dispatch trusts you
//!   with the freight; paying for the course yourself unlocks it early.
//!   Flatbed load securement is real carrier onboarding (the rules are
//!   federal -- 49 CFR 393 subpart I -- but the training is the company's).
//! - **Endorsements** (mid-late): the real CDL letters. T doubles, N tank,
//!   H hazmat -- knowledge tests under 49 CFR 383.93(c), and for H a
//!   fingerprint background check (49 CFR 1572) that takes real weeks:
//!   the endorsement cannot issue until the check clears. Holding both N
//!   and H is the X combination (49 CFR 383.153(a)(9)(v)).
//! - **Specialist** (late): credentials that are not CDL letters at all.
//!   TWIC, the port security card (MTSA; TSA-issued, ~$125, weeks to
//!   clear). LCV, the longer-combination-vehicle certificate -- issued by
//!   the training provider and held in the carrier's file, never by the
//!   DMV (49 CFR 380.107(c)) -- which requires the T endorsement, real
//!   experience, and a clean recent record (49 CFR 380.203).
//!
//! Every credential is stored by key in `Career.purchased_endorsements`
//! (level-granted ones are derived, exactly as before); the catalog is
//! exported to the orinks.net validator, so a new key here is an exporter
//! regen and a staging deploy in the same change.

use crate::pyfmt::fmt_grouped;

/// Which rung of the ladder a credential sits on. The tier picks the
/// spoken noun: "training", "certificate", "endorsement", or the
/// credential's own proper name for specialist cards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialTier {
    Training,
    Certificate,
    Endorsement,
    Specialist,
}

impl CredentialTier {
    /// The tier as the export's stable lowercase string.
    pub fn as_str(self) -> &'static str {
        match self {
            CredentialTier::Training => "training",
            CredentialTier::Certificate => "certificate",
            CredentialTier::Endorsement => "endorsement",
            CredentialTier::Specialist => "specialist",
        }
    }
}

/// One rung of the credential ladder.
#[derive(Debug, Clone, PartialEq)]
pub struct Credential {
    pub key: &'static str,
    pub tier: CredentialTier,
    /// Spoken and public-profile label ("refrigerated", "tank vehicle").
    pub label: &'static str,
    /// The full phrase a locked job names ("refrigerated certificate").
    pub gate_label: &'static str,
    /// The carrier sponsors the training at this level; None means the
    /// credential is only ever earned through the course.
    pub grant_level: Option<i64>,
    /// The earliest level the course can be booked.
    pub min_level: i64,
    pub course_cost: f64,
    /// Time the course takes; the game clock advances by this many hours.
    pub course_hours: f64,
    /// Credentials that must already be held before booking.
    pub prereqs: &'static [&'static str],
    /// Background-check wait, in game days, before the credential
    /// activates after the course (0 = it activates on the spot).
    pub wait_days: f64,
    /// Booking requires a clean recent driving record (no recent serious
    /// violations, no live suspension) -- the 49 CFR 380.203 shape.
    pub needs_clean_record: bool,
    /// What holding it opens, as the freight board names it ("fresh food
    /// and refrigerated goods"); spoken by F1 on the course screen so a
    /// driver knows what a course buys before booking it.
    pub unlocks: &'static str,
    /// Spoken when the credential lands.
    pub announcement: &'static str,
}

/// The ladder, in menu order: act one to act three.
///
/// Costs and waits are rounded from published figures (state test fees,
/// TSA's $85.25 hazmat threat assessment and $124 TWIC fee, its "apply 60
/// days ahead / may exceed 45 days" processing guidance) -- sources in
/// docs/ontology.md's credential rows and the roadmap entry that designed
/// this ladder.
///
/// Buying ahead is bounded, not open (owner rule, 2026-08-30): a sponsored
/// course's `min_level` sits exactly ONE level below its grant level, so
/// money buys a head start of one level per course and never a skip
/// through the ladder -- a rich rookie gets two level-2 certificates and
/// nothing else. The course-only credentials carry their own hard level
/// gates and were never buyable early.
pub const CREDENTIALS: &[Credential] = &[
    // -- Training: restriction removal -------------------------------------
    Credential {
        key: "manual_transmission",
        tier: CredentialTier::Training,
        label: "manual transmission",
        gate_label: "manual transmission training",
        grant_level: None,
        min_level: 1,
        course_cost: 220.0,
        course_hours: 8.0,
        prereqs: &[],
        wait_days: 0.0,
        needs_clean_record: false,
        unlocks: "a small pay bonus for drives in a manual truck",
        announcement: "You retook the skills test in a manual truck, and the automatic-only \
             restriction is off your license. Drives in a manual truck now pay a \
             small bonus at settlement.",
    },
    // -- Certificates: carrier training ------------------------------------
    Credential {
        key: "refrigerated",
        tier: CredentialTier::Certificate,
        label: "refrigerated",
        gate_label: "refrigerated certificate",
        grant_level: Some(2),
        min_level: 1,
        course_cost: 900.0,
        course_hours: 6.0,
        prereqs: &[],
        wait_days: 0.0,
        needs_clean_record: false,
        unlocks: "fresh food and refrigerated goods",
        announcement: "You earned the refrigerated certificate. \
             Food and refrigerated cargo jobs are now available.",
    },
    Credential {
        key: "flatbed_securement",
        tier: CredentialTier::Certificate,
        label: "flatbed securement",
        gate_label: "flatbed securement certificate",
        grant_level: Some(2),
        min_level: 1,
        course_cost: 450.0,
        course_hours: 8.0,
        prereqs: &[],
        wait_days: 0.0,
        needs_clean_record: false,
        unlocks: "steel products, and lumber and paper products",
        announcement: "You earned the flatbed securement certificate. Steel and lumber \
             jobs are now available.",
    },
    Credential {
        key: "heavy_haul",
        tier: CredentialTier::Certificate,
        label: "heavy-haul",
        gate_label: "heavy-haul certificate",
        grant_level: Some(3),
        min_level: 2,
        course_cost: 1_600.0,
        course_hours: 8.0,
        prereqs: &[],
        wait_days: 0.0,
        needs_clean_record: false,
        unlocks: "heavy machinery",
        announcement: "You earned the heavy-haul certificate. Heavy machinery jobs are now \
             available.",
    },
    Credential {
        key: "high_value",
        tier: CredentialTier::Certificate,
        label: "high-value",
        gate_label: "high-value certificate",
        grant_level: Some(4),
        min_level: 3,
        course_cost: 1_300.0,
        course_hours: 6.0,
        prereqs: &[],
        wait_days: 0.0,
        needs_clean_record: false,
        unlocks: "electronics and packaged industrial chemicals",
        announcement: "You earned the high-value certificate. Electronics jobs are now \
             available.",
    },
    // -- Endorsements: the CDL letters -------------------------------------
    Credential {
        key: "doubles_triples",
        tier: CredentialTier::Endorsement,
        label: "doubles",
        gate_label: "doubles endorsement",
        grant_level: None,
        min_level: 8,
        course_cost: 60.0,
        course_hours: 4.0,
        prereqs: &[],
        wait_days: 0.0,
        needs_clean_record: false,
        unlocks: "twin-trailer parcel freight",
        announcement: "You passed the written test for the doubles endorsement, the letter \
             T on a real license. Twin-trailer parcel freight is now available. \
             Two short trailers, legal nationwide, and the rear one wanders.",
    },
    Credential {
        key: "tank",
        tier: CredentialTier::Endorsement,
        label: "tank vehicle",
        gate_label: "tank vehicle endorsement",
        grant_level: Some(16),
        min_level: 15,
        course_cost: 2_400.0,
        course_hours: 6.0,
        prereqs: &[],
        wait_days: 0.0,
        needs_clean_record: false,
        unlocks: "liquid food products from level 21, and bulk fuel once the hazmat endorsement joins it",
        announcement: "You earned the tank vehicle endorsement, the letter N on a real \
             license. Liquid bulk opens from here, bulk fuel once the hazmat \
             endorsement joins it, and liquid food at level 21.",
    },
    Credential {
        key: "hazmat",
        tier: CredentialTier::Endorsement,
        label: "hazmat",
        gate_label: "hazmat endorsement",
        grant_level: None,
        min_level: 10,
        course_cost: 185.0,
        course_hours: 8.0,
        prereqs: &[],
        // TSA tells hazmat applicants to enroll at least 60 days ahead and
        // warns processing may exceed 45; a month of game days keeps the
        // wait real without stalling an act-two career.
        wait_days: 30.0,
        needs_clean_record: false,
        unlocks: "placarded hazardous materials, and bulk fuel once the tank vehicle endorsement joins it",
        announcement: "Your hazmat background check cleared, and the hazmat endorsement, \
             the letter H, is on your license. Placarded chemical freight is now \
             available. Placards mean every scale is watching.",
    },
    // -- Specialist credentials --------------------------------------------
    Credential {
        key: "twic",
        tier: CredentialTier::Specialist,
        label: "TWIC port card",
        gate_label: "TWIC port card",
        grant_level: None,
        min_level: 18,
        course_cost: 125.0,
        course_hours: 4.0,
        prereqs: &[],
        wait_days: 20.0,
        needs_clean_record: false,
        unlocks: "port containers",
        announcement: "Your TWIC enrollment cleared. Container freight out of the port \
             terminals is now available.",
    },
    Credential {
        key: "lcv",
        tier: CredentialTier::Specialist,
        label: "LCV",
        gate_label: "LCV certificate",
        grant_level: None,
        min_level: 20,
        course_cost: 2_000.0,
        course_hours: 24.0,
        prereqs: &["doubles_triples"],
        wait_days: 0.0,
        needs_clean_record: true,
        unlocks: "turnpike doubles freight, between the states whose networks allow them",
        announcement: "You finished LCV training and hold the longer-combination-vehicle \
             certificate. Turnpike doubles are now available, only between the \
             states whose networks allow them.",
    },
];

/// `CREDENTIALS[key]`.
pub fn credential(key: &str) -> Option<&'static Credential> {
    CREDENTIALS.iter().find(|c| c.key == key)
}

/// Every catalog key, in ladder order.
pub fn credential_keys() -> impl Iterator<Item = &'static str> {
    CREDENTIALS.iter().map(|c| c.key)
}

/// Share of a settlement added when the driver trained out of the
/// automatic-only E restriction and ran the whole trip on a manual box.
/// Small on purpose: the restriction removal is a first rung, not a pay
/// raise, and the differential is the carrier valuing a driver who can
/// take any tractor on the yard.
pub const MANUAL_SPEC_DIFFERENTIAL: f64 = 0.03;

/// States whose road networks allow longer combination vehicles, from the
/// ISTEA freeze list (23 CFR 658 Appendix C): the turnpike systems that run
/// doubles (Kansas, Ohio, Indiana, Massachusetts, New York, Florida) plus
/// the western states whose entries authorize multi-trailer combinations.
/// The freeze is real law: no state can be added without Congress, which is
/// why this list is a constant and not a data file.
pub const LCV_STATES: &[&str] = &[
    "AK", "AZ", "CO", "FL", "ID", "IN", "KS", "MA", "MT", "ND", "NE", "NV", "NY", "OH", "OK", "OR",
    "SD", "UT",
];

/// Whether a state's network takes longer combination vehicles.
pub fn lcv_state(state_code: &str) -> bool {
    LCV_STATES.contains(&state_code)
}

/// A course booking judged against everything but money: level, held,
/// pending, prerequisites, record. Returns `(bookable, reasons)` in the
/// weigh-station-transponder shape -- every blocking reason, spoken.
pub fn course_eligibility(
    cred: &Credential,
    level: i64,
    held: &std::collections::BTreeSet<&'static str>,
    pending: &[String],
    clean_record: bool,
) -> (bool, Vec<String>) {
    let mut reasons: Vec<String> = Vec::new();
    if held.contains(cred.key) {
        reasons.push(format!("You already hold the {}.", cred.gate_label));
    }
    if pending.iter().any(|k| k == cred.key) {
        reasons.push(format!(
            "Your {} is already in the works. The background check has not \
             cleared yet.",
            cred.gate_label
        ));
    }
    if level < cred.min_level {
        reasons.push(format!(
            "The school books this course for level {} drivers and up.",
            cred.min_level
        ));
    }
    for prereq in cred.prereqs {
        if !held.contains(prereq) {
            if let Some(needed) = credential(prereq) {
                reasons.push(format!("Requires the {} first.", needed.gate_label));
            }
        }
    }
    if cred.needs_clean_record && !clean_record {
        reasons.push(
            "Requires a clean recent driving record, no recent serious \
             violations and no live suspension."
                .to_string(),
        );
    }
    (reasons.is_empty(), reasons)
}

/// F1 on a course row: what the credential opens, and how it is gained.
///
/// A sponsored certificate names both roads to it: the level the carrier
/// pays at, and the one level earlier a driver may pay for it (owner,
/// 2026-09-18: the row's cost and level said nothing about what the
/// course was FOR). A course-only credential names its level, cost,
/// prerequisites and any background-check wait.
pub fn course_help_text(cred: &Credential) -> String {
    let mut text = format!("Unlocks {}. ", cred.unlocks);
    match cred.grant_level {
        Some(level) => text.push_str(&format!(
            "The carrier sponsors it free at level {level}, or you can pay {} dollars for it \
             yourself from level {}.",
            fmt_grouped(cred.course_cost, 0),
            cred.min_level
        )),
        None => text.push_str(&format!(
            "Course only: {} dollars from level {}.",
            fmt_grouped(cred.course_cost, 0),
            cred.min_level
        )),
    }
    let needs: Vec<String> = cred
        .prereqs
        .iter()
        .filter_map(|key| credential(key))
        .map(|c| format!("the {}", c.gate_label))
        .chain(
            cred.needs_clean_record
                .then(|| "a clean recent record".to_string()),
        )
        .collect();
    if !needs.is_empty() {
        text.push_str(&format!(" Needs {} first.", needs.join(" and ")));
    }
    if cred.wait_days > 0.0 {
        text.push_str(&format!(
            " The background check takes about {} days and clears while you drive.",
            cred.wait_days as i64
        ));
    }
    text.push_str(" Enter books it.");
    text
}

/// The one-line menu row for a course: cost, and what makes it activate.
pub fn course_offer_text(cred: &Credential) -> String {
    let mut text = format!("{} dollars", fmt_grouped(cred.course_cost, 0));
    if cred.wait_days > 0.0 {
        text.push_str(&format!(
            ", background check clears in about {} days",
            cred.wait_days as i64
        ));
    }
    if let Some(level) = cred.grant_level {
        text.push_str(&format!(", carrier-sponsored free at level {level}"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn the_catalog_keys_are_unique_and_prereqs_resolve() {
        let mut seen: Vec<&str> = Vec::new();
        for cred in CREDENTIALS {
            assert!(!seen.contains(&cred.key), "duplicate key {}", cred.key);
            seen.push(cred.key);
            for prereq in cred.prereqs {
                assert!(
                    credential(prereq).is_some(),
                    "{} requires unknown {prereq}",
                    cred.key
                );
            }
            assert!(!cred.announcement.is_empty());
            assert!(!cred.gate_label.is_empty());
        }
    }

    #[test]
    fn the_help_line_says_what_a_course_unlocks_and_both_roads_to_it() {
        let help = course_help_text(credential("refrigerated").unwrap());
        assert_eq!(
            help,
            "Unlocks fresh food and refrigerated goods. The carrier sponsors it free at \
             level 2, or you can pay 900 dollars for it yourself from level 1. Enter books \
             it."
        );
        let help = course_help_text(credential("hazmat").unwrap());
        assert!(
            help.starts_with("Unlocks placarded hazardous materials"),
            "{help}"
        );
        assert!(
            help.contains("Course only: 185 dollars from level 10."),
            "{help}"
        );
        assert!(
            help.contains("background check takes about 30 days"),
            "{help}"
        );
        assert!(!help.contains("sponsors"), "{help}");
        let help = course_help_text(credential("lcv").unwrap());
        assert!(
            help.contains("Needs the doubles endorsement and a clean recent record first."),
            "{help}"
        );
        for cred in CREDENTIALS {
            assert!(
                !cred.unlocks.is_empty(),
                "{} says nothing it unlocks",
                cred.key
            );
        }
    }

    #[test]
    fn the_original_four_keep_their_levels_and_costs() {
        // Save compatibility: these four keys ride existing careers and the
        // deployed invariants export. Their grant levels and course costs
        // are the shipped 1.9 economy.
        for (key, level, cost) in [
            ("refrigerated", 2, 900.0),
            ("heavy_haul", 3, 1_600.0),
            ("high_value", 4, 1_300.0),
            ("tank", 16, 2_400.0),
        ] {
            let cred = credential(key).unwrap();
            assert_eq!(cred.grant_level, Some(level), "{key}");
            assert_eq!(cred.course_cost, cost, "{key}");
        }
    }

    #[test]
    fn money_buys_one_level_of_head_start_and_never_a_skip() {
        // Owner rule, 2026-08-30: a rich rookie must not buy the whole
        // ladder. Every sponsored course books exactly one level early,
        // so at level 1 only the level-2 pair (plus the manual-transmission
        // training, which has no sponsor) is on offer.
        for cred in CREDENTIALS {
            if let Some(grant) = cred.grant_level {
                assert_eq!(cred.min_level, (grant - 1).max(1), "{}", cred.key);
            }
        }
        let held = BTreeSet::new();
        let bookable: Vec<&str> = CREDENTIALS
            .iter()
            .filter(|c| course_eligibility(c, 1, &held, &[], true).0)
            .map(|c| c.key)
            .collect();
        assert_eq!(
            bookable,
            ["manual_transmission", "refrigerated", "flatbed_securement"]
        );
    }

    #[test]
    fn course_only_credentials_never_level_grant() {
        // The letters and cards a carrier cannot hand out: the written test
        // (and for hazmat and TWIC, the federal background check) is the
        // only door. If one of these grew a grant_level, the public profile
        // would credit every driver at that level with a check they never
        // passed.
        for key in [
            "manual_transmission",
            "doubles_triples",
            "hazmat",
            "twic",
            "lcv",
        ] {
            assert_eq!(credential(key).unwrap().grant_level, None, "{key}");
        }
    }

    #[test]
    fn eligibility_speaks_every_blocking_reason() {
        let lcv = credential("lcv").unwrap();
        let held = BTreeSet::new();
        let (ok, reasons) = course_eligibility(lcv, 1, &held, &[], false);
        assert!(!ok);
        // level, prereq, and record all block at once, and all are spoken.
        assert_eq!(reasons.len(), 3);
        assert!(reasons[0].contains("level 20"));
        assert!(reasons[1].contains("doubles endorsement"));
        assert!(reasons[2].contains("clean recent driving record"));

        let mut held = BTreeSet::new();
        held.insert("doubles_triples");
        let (ok, reasons) = course_eligibility(lcv, 20, &held, &[], true);
        assert!(ok, "{reasons:?}");
    }

    #[test]
    fn a_pending_check_blocks_a_second_booking() {
        let hazmat = credential("hazmat").unwrap();
        let held = BTreeSet::new();
        let (ok, reasons) = course_eligibility(hazmat, 12, &held, &["hazmat".to_string()], true);
        assert!(!ok);
        assert!(reasons[0].contains("in the works"));
    }

    #[test]
    fn the_lcv_network_is_the_frozen_list() {
        // Spot checks against 23 CFR 658 Appendix C: the six turnpike-double
        // systems are in; the freeze's famous absentees are out.
        for state in ["KS", "OH", "IN", "MA", "NY", "FL", "MT", "UT"] {
            assert!(lcv_state(state), "{state}");
        }
        for state in [
            "CA", "TX", "IL", "PA", "WY", "IA", "MO", "WA", "NM", "MI", "MS", "HI",
        ] {
            assert!(!lcv_state(state), "{state}");
        }
    }
}
