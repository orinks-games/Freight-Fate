//! Structured 30-level career ladders: the owner-operator arc and the
//! parallel company/fleet arc (port of `freight_fate/models/career_ladder.py`).

use serde::Serialize;

pub const STARTER_CARRIER_NAME: &str = "Northstar Freight Lines";
pub const MAX_CAREER_LEVEL: i64 = 30;

/// One rung of the ladder. A static catalogue entry, so it serialises but is
/// never read back from a save.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CareerRank {
    pub level: i64,
    pub title: &'static str,
    pub stage: &'static str,
    pub unlock: &'static str,
    pub status: &'static str,
}

const fn rank(
    level: i64,
    title: &'static str,
    stage: &'static str,
    unlock: &'static str,
    status: &'static str,
) -> CareerRank {
    CareerRank {
        level,
        title,
        stage,
        unlock,
        status,
    }
}

pub const CAREER_RANKS: [CareerRank; 30] = [
    rank(
        1,
        "Yard Trainee",
        "Company driver",
        "Starter company tractor and short regional dispatches.",
        "Learning company procedures with carrier-paid equipment.",
    ),
    rank(
        2,
        "New Hire Company Driver",
        "Company driver",
        "Refrigerated and flatbed securement certificates from carrier training.",
        "Running short freight with company dispatch and company equipment.",
    ),
    rank(
        3,
        "Solo Company Driver",
        "Company driver",
        "Heavy-haul freight certificate.",
        "Trusted for solo regional work and heavier freight.",
    ),
    rank(
        4,
        "Regional Company Driver",
        "Company driver",
        "High-value freight certificate, and dispatch moves you up to the \
         regional fleet: a few newer spare tractors, with the one that suits \
         each load assigned to you.",
        "Working broader lanes while the carrier still owns the business risk.",
    ),
    rank(
        5,
        "Regional Regular",
        "Company driver",
        "One extra assigned-load decline per promotion, and broader regional lane variety.",
        "A dependable company driver, still on assigned carrier equipment.",
    ),
    rank(
        6,
        "Experienced Company Driver",
        "Company driver",
        "The dispatch board now shows six offers per visit.",
        "Building miles, reputation, and savings on carrier equipment.",
    ),
    rank(
        7,
        "Long-Haul Company Driver",
        "Company driver",
        "Long-haul dispatch becomes routine, with haul-length limits still growing every level.",
        "Trusted for longer routes with company tractor and trailer support.",
    ),
    rank(
        8,
        "Heavy Freight Driver",
        "Company driver",
        "Dispatch now lets you choose your own loads from the board, \
         with more machinery, construction, and bulk opportunities. The \
         doubles endorsement course opens under Licenses and training.",
        "Trusted with heavier freight while the carrier covers operating costs.",
    ),
    rank(
        9,
        "High-Value Company Driver",
        "Company driver",
        "Dispatch assigns you one long-haul sleeper tractor that you drive on \
         every load, with priority access to fragile and high-value lanes.",
        "Dispatch trusts the driver with higher-consequence freight.",
    ),
    rank(
        10,
        "Lead Company Driver",
        "Company driver",
        "Senior company-driver status, the dispatch board grows to seven \
         offers per visit, and the hazmat endorsement course opens under \
         Licenses and training.",
        "A veteran company driver, still protected from tractor operating costs.",
    ),
    rank(
        11,
        "Specialized Company Driver",
        "Senior company driver",
        "Specialized freight now appears more often on your board.",
        "Credentials and careful service matter more to dispatch.",
    ),
    rank(
        12,
        "Premium Lane Driver",
        "Senior company driver",
        "The board grows to eight offers and favors premium long-haul lanes.",
        "Trusted for premium company freight without personal equipment risk.",
    ),
    rank(
        13,
        "Carrier Mentor Driver",
        "Senior company driver",
        "Dispatch upgrades your assigned tractor to a premium fleet unit.",
        "A senior company driver with reliable service history.",
    ),
    rank(
        14,
        "Business Prep Driver",
        "Senior company driver",
        "Business status now reads the full owner-operator checklist.",
        "Learning reserves and settlement risk while still on company wages.",
    ),
    rank(
        15,
        "Owner-Operator Candidate",
        "Business preparation",
        "Working-capital target becomes visible.",
        "Saving for the owner-operator buy-in instead of a lease-purchase deal.",
    ),
    rank(
        16,
        "Leased-On Applicant",
        "Owner-operator preparation",
        "Every owner-operator buy-in requirement appears in full.",
        "Finalizing delivery, reputation, and cash readiness.",
    ),
    rank(
        17,
        "Tractor Buy-In Candidate",
        "Owner-operator preparation",
        "First pick of the yard: dispatch assigns the carrier's best \
         tractor, and the buy-in target is active.",
        "Close to the owner-operator buy-in, but still on company wages.",
    ),
    rank(
        18,
        "Leased-On Owner-Operator",
        "Owner-operator",
        "The owner-operator buy-in opens once its other requirements are met: \
         you buy the truck you drive and haul for the carrier as a leased-on \
         owner-operator. TWIC port card enrollment opens under Licenses and \
         training.",
        "Eligible to buy your own truck and pay its operating costs.",
    ),
    rank(
        19,
        "Settled Owner-Operator",
        "Owner-operator",
        "Owner-operator settlements become the normal business rhythm.",
        "Learning fuel, maintenance, insurance, trailer, and settlement reserves.",
    ),
    rank(
        20,
        "Established Owner-Operator",
        "Owner-operator",
        "Specialty trailer programs matter more, and LCV training opens for \
         doubles-endorsed drivers with a clean recent record.",
        "Running as a steady leased-on business with clearer upside and costs.",
    ),
    rank(
        21,
        "Authority Prep Candidate",
        "Authority preparation",
        "Authority prep reserve can unlock when other gates are met.",
        "Studying full authority while staying focused on one-truck operations.",
    ),
    rank(
        22,
        "Direct Freight Prep",
        "Authority preparation",
        "Direct freight readiness gates become clearer.",
        "Building reputation and working capital before direct broker freight.",
    ),
    rank(
        23,
        "Trailer Strategy Owner",
        "Authority preparation",
        "Trailer ownership planning matters for direct freight.",
        "Choosing when a trailer program is enough and when ownership helps.",
    ),
    rank(
        24,
        "Authority-Ready Operator",
        "Authority preparation",
        "Final authority activation checklist.",
        "Prepared for own authority without simulating a full compliance office.",
    ),
    rank(
        25,
        "Independent Authority Operator",
        "Own authority",
        "Own authority and direct freight unlock when other gates are met.",
        "Direct freight is available with insurance, compliance, and factoring costs.",
    ),
    rank(
        26,
        "Contract Freight Builder",
        "Established owner-operator",
        "Premium direct freight reputation matters more.",
        "Building a stronger one-truck book of repeat freight.",
    ),
    rank(
        27,
        "Specialized Trailer Operator",
        "Established owner-operator",
        "Specialized trailer opportunities stand out.",
        "Matching owned or leased equipment to better freight.",
    ),
    rank(
        28,
        "Premium Lane Operator",
        "Established owner-operator",
        "Premium lanes favor high reputation and the right trailer.",
        "An established owner-operator with strong freight-market choice.",
    ),
    rank(
        29,
        "Veteran Independent Operator",
        "Established owner-operator",
        "Prestige freight and best dispatch quality.",
        "A proven independent driver, not a fleet manager.",
    ),
    rank(
        30,
        "Freight Fate Independent",
        "Established owner-operator",
        "Top career rank.",
        "A complete company-driver to independent owner-operator career.",
    ),
];

/// Parallel ladder for drivers who stay on (or return to) company payroll.
/// Levels 1-14 match [`CAREER_RANKS`]; from 15 on the titles and goals are
/// senior company / fleet ranks instead of the owner-operator arc.
pub const COMPANY_CAREER_RANKS: [CareerRank; 30] = [
    rank(
        1,
        "Yard Trainee",
        "Company driver",
        "Starter company tractor and short regional dispatches.",
        "Learning company procedures with carrier-paid equipment.",
    ),
    rank(
        2,
        "New Hire Company Driver",
        "Company driver",
        "Refrigerated and flatbed securement certificates from carrier training.",
        "Running short freight with company dispatch and company equipment.",
    ),
    rank(
        3,
        "Solo Company Driver",
        "Company driver",
        "Heavy-haul freight certificate.",
        "Trusted for solo regional work and heavier freight.",
    ),
    rank(
        4,
        "Regional Company Driver",
        "Company driver",
        "High-value freight certificate, and dispatch moves you up to the \
         regional fleet: a few newer spare tractors, with the one that suits \
         each load assigned to you.",
        "Working broader lanes while the carrier still owns the business risk.",
    ),
    rank(
        5,
        "Regional Regular",
        "Company driver",
        "One extra assigned-load decline per promotion, and broader regional lane variety.",
        "A dependable company driver, still on assigned carrier equipment.",
    ),
    rank(
        6,
        "Experienced Company Driver",
        "Company driver",
        "The dispatch board now shows six offers per visit.",
        "Building miles, reputation, and savings on carrier equipment.",
    ),
    rank(
        7,
        "Long-Haul Company Driver",
        "Company driver",
        "Long-haul dispatch becomes routine, with haul-length limits still growing every level.",
        "Trusted for longer routes with company tractor and trailer support.",
    ),
    rank(
        8,
        "Heavy Freight Driver",
        "Company driver",
        "Dispatch now lets you choose your own loads from the board, \
         with more machinery, construction, and bulk opportunities. The \
         doubles endorsement course opens under Licenses and training.",
        "Trusted with heavier freight while the carrier covers operating costs.",
    ),
    rank(
        9,
        "High-Value Company Driver",
        "Company driver",
        "Dispatch assigns you one long-haul sleeper tractor that you drive on \
         every load, with priority access to fragile and high-value lanes.",
        "Dispatch trusts the driver with higher-consequence freight.",
    ),
    rank(
        10,
        "Lead Company Driver",
        "Company driver",
        "Senior company-driver status, the dispatch board grows to seven \
         offers per visit, and the hazmat endorsement course opens under \
         Licenses and training.",
        "A veteran company driver, still protected from tractor operating costs.",
    ),
    rank(
        11,
        "Specialized Company Driver",
        "Senior company driver",
        "Specialized freight now appears more often on your board.",
        "Credentials and careful service matter more to dispatch.",
    ),
    rank(
        12,
        "Premium Lane Driver",
        "Senior company driver",
        "The board grows to eight offers and favors premium long-haul lanes.",
        "Trusted for premium company freight without personal equipment risk.",
    ),
    rank(
        13,
        "Carrier Mentor Driver",
        "Senior company driver",
        "Dispatch upgrades your assigned tractor to a premium fleet unit.",
        "A senior company driver with reliable service history.",
    ),
    rank(
        14,
        "Business Prep Driver",
        "Senior company driver",
        "Business status now reads the full owner-operator checklist.",
        "Learning reserves and settlement risk while still on company wages.",
    ),
    rank(
        15,
        "Dedicated Company Driver",
        "Career company driver",
        "Senior company lanes stay open without the owner-operator prep track.",
        "Choosing carrier equipment as a long-term career, not a waiting room.",
    ),
    rank(
        16,
        "Fleet Senior Driver",
        "Career company driver",
        "Dispatch favors you with steadier premium freight on company iron.",
        "A senior company driver with a settled place on the fleet.",
    ),
    rank(
        17,
        "Lead Fleet Driver",
        "Career company driver",
        "First pick of the yard: dispatch assigns the carrier's best tractor \
         for a company driver who stayed on the payroll.",
        "Top of the assigned-fleet ladder while the carrier still owns the risk.",
    ),
    rank(
        18,
        "Company Fleet Captain",
        "Career company driver",
        "Fleet-captain standing: priority company freight and mentoring weight.",
        "A career company driver at the buy-in fork who kept the safer seat.",
    ),
    rank(
        19,
        "Veteran Company Hauler",
        "Career company driver",
        "Long-haul company work becomes the normal rhythm.",
        "Miles and reputation compound on carrier equipment.",
    ),
    rank(
        20,
        "Senior Fleet Regular",
        "Career company driver",
        "Specialty company freight and clean-service bonuses matter more.",
        "A dependable senior company driver with nothing left to prove on risk.",
    ),
    rank(
        21,
        "Carrier Elite Driver",
        "Elite company driver",
        "Elite company standing opens the strongest carrier lanes.",
        "Running the freight the carrier trusts only its best drivers with.",
    ),
    rank(
        22,
        "Premium Company Lane Driver",
        "Elite company driver",
        "Premium company lanes favor high reputation and clean timing.",
        "An elite company driver with first call on high-consequence freight.",
    ),
    rank(
        23,
        "Master Company Driver",
        "Elite company driver",
        "Master company status: mentoring weight and priority board placement.",
        "Credentials, service, and seniority carry equal weight with dispatch.",
    ),
    rank(
        24,
        "Distinguished Fleet Driver",
        "Elite company driver",
        "Distinguished fleet standing across the carrier's best lanes.",
        "A distinguished company driver still protected from equipment risk.",
    ),
    rank(
        25,
        "Senior Fleet Mentor",
        "Lifetime company driver",
        "Senior mentoring status: the carrier leans on your judgment.",
        "Teaching by example while still driving company freight.",
    ),
    rank(
        26,
        "Carrier Lifetime Driver",
        "Lifetime company driver",
        "Lifetime company standing with the strongest repeat lanes.",
        "A career measured in clean company miles, not ownership.",
    ),
    rank(
        27,
        "Elite Specialized Company Driver",
        "Lifetime company driver",
        "Specialized company freight stands out on your board.",
        "Matching credentials to the carrier's hardest work.",
    ),
    rank(
        28,
        "Top-Seniority Company Driver",
        "Lifetime company driver",
        "Top-seniority company placement on prestige carrier lanes.",
        "Nothing on the board is above your standing.",
    ),
    rank(
        29,
        "Company Career Veteran",
        "Lifetime company driver",
        "Prestige company freight and best dispatch quality.",
        "A proven company driver at the top of the carrier ladder.",
    ),
    rank(
        30,
        "Freight Fate Company Driver",
        "Lifetime company driver",
        "Top company-driver career rank.",
        "A complete company-driver career without taking the owner-operator seat.",
    ),
];

/// First level whose default [`CAREER_RANKS`] title is the leased-on
/// owner-operator rank. Company drivers at or past this level use
/// [`COMPANY_CAREER_RANKS`] unless they take the owner-operator path.
/// Kept in lockstep with `business::OWNER_OPERATOR_LEVEL`.
pub const COMPANY_RANK_FORK_LEVEL: i64 = 18;

pub fn rank_for_level(level: i64) -> &'static CareerRank {
    let clamped = level.clamp(1, MAX_CAREER_LEVEL);
    &CAREER_RANKS[(clamped - 1) as usize]
}

pub fn next_rank_for_level(level: i64) -> Option<&'static CareerRank> {
    if level >= MAX_CAREER_LEVEL {
        return None;
    }
    Some(rank_for_level(level + 1))
}

pub fn company_rank_for_level(level: i64) -> &'static CareerRank {
    let clamped = level.clamp(1, MAX_CAREER_LEVEL);
    &COMPANY_CAREER_RANKS[(clamped - 1) as usize]
}

pub fn next_company_rank_for_level(level: i64) -> Option<&'static CareerRank> {
    if level >= MAX_CAREER_LEVEL {
        return None;
    }
    Some(company_rank_for_level(level + 1))
}

/// Company payroll titles when the driver declined the buy-in, or is still on
/// carrier wages at or past the owner-operator fork (returned / repossessed /
/// never bought in). Owner-operators keep [`CAREER_RANKS`].
pub fn uses_company_career_ranks(
    level: i64,
    business_status: &str,
    owner_operator_declined: bool,
) -> bool {
    use crate::models::business_constants::is_owner_operator;
    if is_owner_operator(business_status) {
        return false;
    }
    if owner_operator_declined {
        return true;
    }
    level >= COMPANY_RANK_FORK_LEVEL
}

pub fn display_rank_for_level(
    level: i64,
    business_status: &str,
    owner_operator_declined: bool,
) -> &'static CareerRank {
    if uses_company_career_ranks(level, business_status, owner_operator_declined) {
        company_rank_for_level(level)
    } else {
        rank_for_level(level)
    }
}

/// Rewrite level-up lines so a company-path driver hears company titles.
pub fn relabel_level_up_announcements(
    messages: &mut [String],
    from_level: i64,
    to_level: i64,
    use_company_ranks: bool,
) {
    if !use_company_ranks || from_level >= to_level {
        return;
    }
    for level in (from_level + 1)..=to_level {
        let oo = rank_for_level(level);
        let company = company_rank_for_level(level);
        if oo.title == company.title && oo.unlock == company.unlock {
            continue;
        }
        let from = format!("level {level}: {}. Unlock: {}", oo.title, oo.unlock);
        let to = format!(
            "level {level}: {}. Unlock: {}",
            company.title, company.unlock
        );
        for message in messages.iter_mut() {
            if message.starts_with("Level up!") {
                *message = message.replace(&from, &to);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Ported from the ladder cases of `tests/test_career_unlocks.py`,
    //! `tests/test_business_arc.py` and `tests/test_models.py`.

    use super::*;

    #[test]
    fn test_every_rank_describes_a_real_unlock() {
        assert_eq!(CAREER_RANKS.len() as i64, MAX_CAREER_LEVEL);
        assert_eq!(COMPANY_CAREER_RANKS.len() as i64, MAX_CAREER_LEVEL);
        assert_eq!(MAX_CAREER_LEVEL, 30);
        for rank in CAREER_RANKS.iter().chain(COMPANY_CAREER_RANKS.iter()) {
            assert!(!rank.unlock.trim().is_empty());
            assert!(!rank.status.trim().is_empty());
        }
    }

    #[test]
    fn test_fleet_tier_boundaries_are_named_in_the_ladder() {
        // Dispatch upgrades the assigned tractor at these ranks; the level-up
        // announcement reads the unlock text, so the text must say so.
        for level in [4usize, 9, 13, 17] {
            assert!(CAREER_RANKS[level - 1]
                .unlock
                .to_lowercase()
                .contains("tractor"));
        }
    }

    #[test]
    fn test_thirty_level_ladder_has_business_arc_titles() {
        assert_eq!(CAREER_RANKS.len(), 30);
        let levels: Vec<i64> = CAREER_RANKS.iter().map(|r| r.level).collect();
        assert_eq!(levels, (1..=30).collect::<Vec<i64>>());
        assert_eq!(CAREER_RANKS[0].title, "Yard Trainee");
        assert_eq!(CAREER_RANKS[4].title, "Regional Regular");
        assert_eq!(CAREER_RANKS[14].title, "Owner-Operator Candidate");
        assert_eq!(CAREER_RANKS[17].title, "Leased-On Owner-Operator");
        assert_eq!(CAREER_RANKS[24].title, "Independent Authority Operator");
        assert_eq!(CAREER_RANKS[29].title, "Freight Fate Independent");
    }

    #[test]
    fn test_level_thresholds() {
        // The rank half of tests/test_models.py::test_level_thresholds; the
        // level_for_xp half belongs to models::career.
        assert_eq!(rank_for_level(20).title, "Established Owner-Operator");
        assert_eq!(
            rank_for_level(MAX_CAREER_LEVEL).title,
            "Freight Fate Independent"
        );
    }

    #[test]
    fn rank_lookup_clamps_and_next_rank_stops_at_the_top() {
        assert_eq!(rank_for_level(0).level, 1);
        assert_eq!(rank_for_level(-5).level, 1);
        assert_eq!(rank_for_level(99).level, 30);
        assert_eq!(next_rank_for_level(1).unwrap().level, 2);
        assert_eq!(next_rank_for_level(29).unwrap().level, 30);
        assert!(next_rank_for_level(30).is_none());
        assert!(next_rank_for_level(31).is_none());
        assert_eq!(STARTER_CARRIER_NAME, "Northstar Freight Lines");
    }

    #[test]
    fn the_long_unlock_lines_kept_their_spaces_across_the_joins() {
        assert!(CAREER_RANKS[3]
            .unlock
            .contains("up to the regional fleet: a few newer spare tractors, with"));
        assert!(CAREER_RANKS[7]
            .unlock
            .contains("from the board, with more machinery"));
        assert!(CAREER_RANKS[8]
            .unlock
            .contains("you drive on every load, with priority"));
        assert!(CAREER_RANKS[16]
            .unlock
            .contains("carrier's best tractor, and the"));
        assert!(CAREER_RANKS[17]
            .unlock
            .contains("leased-on owner-operator. TWIC port card"));
    }

    #[test]
    fn company_ladder_diverges_from_owner_operator_titles_past_prep() {
        assert_eq!(COMPANY_CAREER_RANKS.len(), 30);
        assert_eq!(COMPANY_RANK_FORK_LEVEL, 18);
        for level in 1..=14 {
            assert_eq!(
                company_rank_for_level(level).title,
                rank_for_level(level).title,
                "level {level} should match on both ladders"
            );
        }
        assert_eq!(company_rank_for_level(15).title, "Dedicated Company Driver");
        assert_eq!(company_rank_for_level(18).title, "Company Fleet Captain");
        assert_eq!(company_rank_for_level(25).title, "Senior Fleet Mentor");
        assert_eq!(
            company_rank_for_level(30).title,
            "Freight Fate Company Driver"
        );
        assert_ne!(company_rank_for_level(18).title, rank_for_level(18).title);
        assert_ne!(company_rank_for_level(30).title, rank_for_level(30).title);
        assert!(COMPANY_CAREER_RANKS[16]
            .unlock
            .to_lowercase()
            .contains("tractor"));
    }

    #[test]
    fn display_rank_follows_business_path_and_declined_choice() {
        use crate::models::business_constants::{
            COMPANY_DRIVER, INDEPENDENT_AUTHORITY, LEASED_OWNER_OPERATOR,
        };
        assert!(!uses_company_career_ranks(16, COMPANY_DRIVER, false));
        assert_eq!(
            display_rank_for_level(16, COMPANY_DRIVER, false).title,
            "Leased-On Applicant"
        );
        assert!(uses_company_career_ranks(16, COMPANY_DRIVER, true));
        assert_eq!(
            display_rank_for_level(16, COMPANY_DRIVER, true).title,
            "Fleet Senior Driver"
        );
        assert!(uses_company_career_ranks(20, COMPANY_DRIVER, false));
        assert_eq!(
            display_rank_for_level(20, COMPANY_DRIVER, false).title,
            "Senior Fleet Regular"
        );
        assert!(!uses_company_career_ranks(20, LEASED_OWNER_OPERATOR, true));
        assert_eq!(
            display_rank_for_level(20, LEASED_OWNER_OPERATOR, true).title,
            "Established Owner-Operator"
        );
        assert!(!uses_company_career_ranks(30, INDEPENDENT_AUTHORITY, false));
        assert_eq!(
            display_rank_for_level(30, INDEPENDENT_AUTHORITY, false).title,
            "Freight Fate Independent"
        );
    }

    #[test]
    fn relabel_rewrites_only_company_path_level_ups() {
        let mut messages = vec![format!(
            "Level up! You are now level 18: Leased-On Owner-Operator. Unlock: {}",
            CAREER_RANKS[17].unlock
        )];
        relabel_level_up_announcements(&mut messages, 17, 18, true);
        assert!(
            messages[0].contains("Company Fleet Captain"),
            "{}",
            messages[0]
        );
        assert!(
            !messages[0].contains("Leased-On Owner-Operator"),
            "{}",
            messages[0]
        );
    }
}
