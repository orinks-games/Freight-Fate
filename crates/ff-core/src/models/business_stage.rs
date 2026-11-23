//! The ownership ladder as one state machine: company driver, the buy-in,
//! leased on, the authority prep reserve, own authority, and the two ways
//! back down.
//!
//! The saved form is still three profile fields that move together --
//! `business_status`, `authority_readiness` and `owner_operator_declined` --
//! so old saves load unchanged. `BusinessStage::from_fields` reads them and
//! `BusinessStage::fields` is the one place that says what each stage
//! writes. Eligibility (`business::owner_operator_eligibility` and the two
//! authority checks) guards an event before it is applied; `next` is only the
//! table of legal moves.
//!
//! Which carrier the driver drives for (hired, let go, on sufferance) is a
//! separate machine, Employment, and is not modelled here.

use crate::models::business_constants::{
    COMPANY_DRIVER, INDEPENDENT_AUTHORITY, LEASED_OWNER_OPERATOR,
};

/// Where the driver stands on the ownership ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BusinessStage {
    /// On a payroll, the buy-in not refused.
    CompanyDriver,
    /// On a payroll, said no to the buy-in (or came back from owning).
    CompanyByChoice,
    /// Owns the tractor, leased to a carrier.
    LeasedOn,
    /// Leased on, with the authority prep reserve set aside.
    LeasedOnPrepared,
    /// Runs under their own operating authority.
    OwnAuthority,
}

/// What moves the driver along the ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BusinessEvent {
    /// "Stay a company driver" under Business status.
    DeclineBuyIn,
    /// "Reopen the owner-operator plan".
    ReopenBuyIn,
    /// The owner-operator buy-in.
    BuyIn,
    /// "Commit ... dollars to authority prep".
    SetAuthorityReserve,
    /// "Activate own authority".
    ActivateAuthority,
    /// "Go back to company driving": every tractor handed back by choice.
    ReturnToCompany,
    /// The lender took the tractor back.
    Repossessed,
}

pub const ALL_STAGES: [BusinessStage; 5] = [
    BusinessStage::CompanyDriver,
    BusinessStage::CompanyByChoice,
    BusinessStage::LeasedOn,
    BusinessStage::LeasedOnPrepared,
    BusinessStage::OwnAuthority,
];

pub const ALL_EVENTS: [BusinessEvent; 7] = [
    BusinessEvent::DeclineBuyIn,
    BusinessEvent::ReopenBuyIn,
    BusinessEvent::BuyIn,
    BusinessEvent::SetAuthorityReserve,
    BusinessEvent::ActivateAuthority,
    BusinessEvent::ReturnToCompany,
    BusinessEvent::Repossessed,
];

/// The three saved profile fields a stage is stored as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageFields {
    pub business_status: &'static str,
    pub authority_readiness: bool,
    pub owner_operator_declined: bool,
}

impl BusinessStage {
    /// The stage this event moves to, or `None` when the move is not legal
    /// from here (nothing changes then).
    pub fn next(self, event: BusinessEvent) -> Option<BusinessStage> {
        use BusinessEvent as E;
        use BusinessStage as S;
        match (self, event) {
            (S::CompanyDriver, E::DeclineBuyIn) => Some(S::CompanyByChoice),
            (S::CompanyByChoice, E::ReopenBuyIn) => Some(S::CompanyDriver),
            (S::CompanyDriver | S::CompanyByChoice, E::BuyIn) => Some(S::LeasedOn),
            (S::LeasedOn, E::SetAuthorityReserve) => Some(S::LeasedOnPrepared),
            (S::LeasedOnPrepared, E::ActivateAuthority) => Some(S::OwnAuthority),
            // Going back by choice is the same choice as "Stay a company
            // driver": titles and goals follow the company ladder after it.
            (S::LeasedOn | S::LeasedOnPrepared | S::OwnAuthority, E::ReturnToCompany) => {
                Some(S::CompanyByChoice)
            }
            (S::LeasedOn | S::LeasedOnPrepared | S::OwnAuthority, E::Repossessed) => {
                Some(S::CompanyDriver)
            }
            _ => None,
        }
    }

    /// The key the agent server and scenarios use.
    pub fn key(self) -> &'static str {
        match self {
            BusinessStage::CompanyDriver => "company_driver",
            BusinessStage::CompanyByChoice => "company_by_choice",
            BusinessStage::LeasedOn => "leased_on",
            BusinessStage::LeasedOnPrepared => "leased_on_prepared",
            BusinessStage::OwnAuthority => "own_authority",
        }
    }

    pub fn from_key(key: &str) -> Option<BusinessStage> {
        ALL_STAGES.into_iter().find(|stage| stage.key() == key)
    }

    pub fn is_owner_operator(self) -> bool {
        matches!(
            self,
            BusinessStage::LeasedOn | BusinessStage::LeasedOnPrepared | BusinessStage::OwnAuthority
        )
    }

    /// Read the stage from the saved fields. The status decides first; the
    /// two flags refine it only where they mean something.
    pub fn from_fields(
        business_status: &str,
        authority_readiness: bool,
        owner_operator_declined: bool,
    ) -> BusinessStage {
        match business_status {
            INDEPENDENT_AUTHORITY => BusinessStage::OwnAuthority,
            LEASED_OWNER_OPERATOR if authority_readiness => BusinessStage::LeasedOnPrepared,
            LEASED_OWNER_OPERATOR => BusinessStage::LeasedOn,
            _ if owner_operator_declined => BusinessStage::CompanyByChoice,
            _ => BusinessStage::CompanyDriver,
        }
    }

    /// What this stage writes to the saved fields. Own authority keeps the
    /// prep reserve flag set, as activating it always has.
    pub fn fields(self) -> StageFields {
        let (business_status, authority_readiness, owner_operator_declined) = match self {
            BusinessStage::CompanyDriver => (COMPANY_DRIVER, false, false),
            BusinessStage::CompanyByChoice => (COMPANY_DRIVER, false, true),
            BusinessStage::LeasedOn => (LEASED_OWNER_OPERATOR, false, false),
            BusinessStage::LeasedOnPrepared => (LEASED_OWNER_OPERATOR, true, false),
            BusinessStage::OwnAuthority => (INDEPENDENT_AUTHORITY, true, false),
        };
        StageFields {
            business_status,
            authority_readiness,
            owner_operator_declined,
        }
    }
}

/// Apply `event` to three saved fields in place: the shared writer behind
/// `Profile::apply_business_event` and the test profiles. Returns the new
/// stage, or `None` (fields untouched) when the move is not legal.
pub fn apply_event_to_fields(
    business_status: &mut String,
    authority_readiness: &mut bool,
    owner_operator_declined: &mut bool,
    event: BusinessEvent,
) -> Option<BusinessStage> {
    let stage = BusinessStage::from_fields(
        business_status,
        *authority_readiness,
        *owner_operator_declined,
    )
    .next(event)?;
    write_stage_to_fields(
        business_status,
        authority_readiness,
        owner_operator_declined,
        stage,
    );
    Some(stage)
}

/// Write a stage to the three saved fields, whatever they held.
pub fn write_stage_to_fields(
    business_status: &mut String,
    authority_readiness: &mut bool,
    owner_operator_declined: &mut bool,
    stage: BusinessStage,
) {
    let fields = stage.fields();
    *business_status = fields.business_status.to_string();
    *authority_readiness = fields.authority_readiness;
    *owner_operator_declined = fields.owner_operator_declined;
}

#[cfg(test)]
mod tests {
    use super::*;
    use BusinessEvent as E;
    use BusinessStage as S;

    /// Every legal move. Any (stage, event) pair not listed is refused.
    const LEGAL: &[(S, E, S)] = &[
        (S::CompanyDriver, E::DeclineBuyIn, S::CompanyByChoice),
        (S::CompanyByChoice, E::ReopenBuyIn, S::CompanyDriver),
        (S::CompanyDriver, E::BuyIn, S::LeasedOn),
        (S::CompanyByChoice, E::BuyIn, S::LeasedOn),
        (S::LeasedOn, E::SetAuthorityReserve, S::LeasedOnPrepared),
        (S::LeasedOnPrepared, E::ActivateAuthority, S::OwnAuthority),
        (S::LeasedOn, E::ReturnToCompany, S::CompanyByChoice),
        (S::LeasedOnPrepared, E::ReturnToCompany, S::CompanyByChoice),
        (S::OwnAuthority, E::ReturnToCompany, S::CompanyByChoice),
        (S::LeasedOn, E::Repossessed, S::CompanyDriver),
        (S::LeasedOnPrepared, E::Repossessed, S::CompanyDriver),
        (S::OwnAuthority, E::Repossessed, S::CompanyDriver),
    ];

    #[test]
    fn every_stage_and_event_pair_follows_the_table() {
        for stage in ALL_STAGES {
            for event in ALL_EVENTS {
                let expected = LEGAL
                    .iter()
                    .find(|(from, on, _)| *from == stage && *on == event)
                    .map(|(_, _, to)| *to);
                assert_eq!(stage.next(event), expected, "{} on {event:?}", stage.key());
            }
        }
    }

    #[test]
    fn keys_round_trip() {
        for stage in ALL_STAGES {
            assert_eq!(BusinessStage::from_key(stage.key()), Some(stage));
        }
        assert_eq!(BusinessStage::from_key("leased"), None);
    }

    #[test]
    fn fields_round_trip_through_the_saved_form() {
        for stage in ALL_STAGES {
            let f = stage.fields();
            assert_eq!(
                BusinessStage::from_fields(
                    f.business_status,
                    f.authority_readiness,
                    f.owner_operator_declined
                ),
                stage
            );
        }
    }

    #[test]
    fn a_refused_event_changes_no_field() {
        let mut status = COMPANY_DRIVER.to_string();
        let mut ready = false;
        let mut declined = false;
        assert_eq!(
            apply_event_to_fields(&mut status, &mut ready, &mut declined, E::ActivateAuthority),
            None
        );
        assert_eq!(
            (status.as_str(), ready, declined),
            (COMPANY_DRIVER, false, false)
        );
    }

    #[test]
    fn old_saves_read_as_the_stage_they_were() {
        // A declined flag left on an owner-operator save means nothing.
        assert_eq!(
            BusinessStage::from_fields(LEASED_OWNER_OPERATOR, false, true),
            S::LeasedOn
        );
        // Own authority with the reserve flag cleared is still own authority.
        assert_eq!(
            BusinessStage::from_fields(INDEPENDENT_AUTHORITY, false, false),
            S::OwnAuthority
        );
        // An unknown status reads as a company driver, as `is_owner_operator`
        // already treats it.
        assert_eq!(
            BusinessStage::from_fields("", false, false),
            S::CompanyDriver
        );
    }
}
