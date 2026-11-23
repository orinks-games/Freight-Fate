//! The ownership ladder on a `Profile`: the one reader and the one writer of
//! the three saved fields `BusinessStage` is stored as.

use super::Profile;
use crate::models::business_stage::{
    apply_event_to_fields, write_stage_to_fields, BusinessEvent, BusinessStage,
};

impl Profile {
    /// Where the driver stands on the ownership ladder.
    pub fn business_stage(&self) -> BusinessStage {
        BusinessStage::from_fields(
            &self.business_status,
            self.authority_readiness,
            self.owner_operator_declined,
        )
    }

    /// Move along the ladder. Sets all three saved fields, or none when the
    /// move is not legal from the current stage.
    /// Leaving ownership, by choice or by repossession, clears what the
    /// buy-in paid: nothing owned is left for it to describe.
    pub fn apply_business_event(&mut self, event: BusinessEvent) -> Option<BusinessStage> {
        let stage = apply_event_to_fields(
            &mut self.business_status,
            &mut self.authority_readiness,
            &mut self.owner_operator_declined,
            event,
        )?;
        if !stage.is_owner_operator() {
            self.buy_in_paid = 0.0;
        }
        Some(stage)
    }

    /// Put the driver at a stage directly: a career start and the sandbox
    /// scenarios, which stage a situation rather than play into it.
    pub fn set_business_stage(&mut self, stage: BusinessStage) {
        write_stage_to_fields(
            &mut self.business_status,
            &mut self.authority_readiness,
            &mut self.owner_operator_declined,
            stage,
        );
    }
}
